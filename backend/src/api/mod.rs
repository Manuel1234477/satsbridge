use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, Method, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    error::{AppError, AppResult},
    security::constant_time_eq,
    services::users,
    state::AppState,
};

mod admin;
mod currencies;
mod dev;
mod lnurl;
mod payments;
mod quotes;
mod recipients;
mod users_api;
mod webhooks;

/// The authenticated caller, set by [`require_user`].
#[derive(Clone, Debug)]
pub struct CurrentUser(pub String);

pub fn router(state: AppState) -> Router {
    let user = Router::new()
        .route("/quotes", post(quotes::create))
        .route("/quotes/{id}", get(quotes::get))
        .route("/payments", post(payments::create))
        .route("/payments/{id}", get(payments::get))
        .route("/payments/{id}/pay", post(payments::pay))
        .route("/payments/{id}/refund", post(payments::refund))
        .route("/transactions", get(payments::list))
        .route("/recipients/resolve", post(recipients::resolve));
    let user = if state.config.dev_routes {
        user.merge(dev::routes())
    } else {
        user
    };
    let user = user.route_layer(middleware::from_fn_with_state(state.clone(), require_user));

    let signup = Router::new()
        .route("/users", post(users_api::create))
        .route_layer(middleware::from_fn_with_state(state.clone(), limit_signups));

    let public = Router::new()
        .route("/health", get(health))
        .route("/currencies", get(currencies::list))
        .route("/payout-institutions", get(recipients::institutions))
        .route_layer(middleware::from_fn_with_state(state.clone(), limit_by_ip));

    // Signed or verified by their own mechanism; not rate limited so provider retries get through.
    let webhooks = Router::new()
        .route("/webhooks/provider", post(webhooks::provider))
        .route("/webhooks/bitnob", post(webhooks::bitnob))
        .route("/webhooks/lnbits", post(webhooks::lnbits));

    // Called by senders' wallets, from any origin (LUD-01).
    let lnurl = Router::new()
        .route("/lnurlp/{id}", get(lnurl::pay_request))
        .route("/lnurlp/{id}/callback", get(lnurl::callback))
        .route_layer(middleware::from_fn_with_state(state.clone(), limit_by_ip))
        .layer(CorsLayer::permissive());

    let mut api = user.merge(signup).merge(public).merge(webhooks).merge(lnurl);
    if state.config.admin_api_key.is_some() {
        api = api.merge(admin::routes().route_layer(middleware::from_fn_with_state(state.clone(), require_admin)));
    }

    let mut app = Router::new().nest("/api/v1", api);
    if let Some(cors) = cors_layer(&state) {
        app = app.layer(cors);
    }
    app.layer(TraceLayer::new_for_http()).with_state(state)
}

fn cors_layer(state: &AppState) -> Option<CorsLayer> {
    let origins: Vec<HeaderValue> = state
        .config
        .cors_allowed_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    if origins.is_empty() {
        return state.config.dev_routes.then(CorsLayer::permissive);
    }
    Some(
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
    )
}

async fn health(State(state): State<AppState>) -> AppResult<Json<Value>> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "lightning": state.lightning.name(),
        "lnurl": state.config.public_base_url.is_some(),
    })))
}

fn bearer(req: &Request) -> Option<&str> {
    req.headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

fn client_ip(state: &AppState, req: &Request) -> String {
    if state.config.trust_proxy_headers
        && let Some(ip) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
    {
        return ip.trim().to_string();
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Resolves the bearer token to a user and applies the per-user rate limit.
async fn require_user(State(state): State<AppState>, mut req: Request, next: Next) -> Result<Response, AppError> {
    let token = bearer(&req).ok_or(AppError::Unauthorized)?;
    let user = users::authenticate(&state, token)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if !state.limits.api.check(&format!("user:{}", user.id)) {
        return Err(AppError::RateLimited);
    }
    req.extensions_mut().insert(CurrentUser(user.id));
    Ok(next.run(req).await)
}

async fn require_admin(State(state): State<AppState>, req: Request, next: Next) -> Result<Response, AppError> {
    let expected = state.config.admin_api_key.as_deref().ok_or(AppError::Unauthorized)?;
    match bearer(&req) {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => Ok(next.run(req).await),
        _ => Err(AppError::Unauthorized),
    }
}

async fn limit_by_ip(State(state): State<AppState>, req: Request, next: Next) -> Result<Response, AppError> {
    if !state.limits.api.check(&format!("ip:{}", client_ip(&state, &req))) {
        return Err(AppError::RateLimited);
    }
    Ok(next.run(req).await)
}

async fn limit_signups(State(state): State<AppState>, req: Request, next: Next) -> Result<Response, AppError> {
    if !state.limits.signups.check(&client_ip(&state, &req)) {
        return Err(AppError::RateLimited);
    }
    Ok(next.run(req).await)
}
