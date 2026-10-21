use axum::{
    Json, Router,
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    error::{AppError, AppResult},
    security::constant_time_eq,
    state::AppState,
};

mod currencies;
mod dev;
mod payments;
mod quotes;
mod recipients;
mod webhooks;

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/quotes", post(quotes::create))
        .route("/quotes/{id}", get(quotes::get))
        .route("/payments", post(payments::create))
        .route("/payments/{id}", get(payments::get))
        .route("/payments/{id}/pay", post(payments::pay))
        .route("/transactions", get(payments::list))
        .route("/recipients/resolve", post(recipients::resolve));
    let protected = if state.config.dev_routes {
        protected.merge(dev::routes())
    } else {
        protected
    };
    let protected = protected.route_layer(middleware::from_fn_with_state(state.clone(), require_api_key));

    // Webhooks authenticate with HMAC signatures instead of the API key.
    let public = Router::new()
        .route("/health", get(health))
        .route("/currencies", get(currencies::list))
        .route("/payout-institutions", get(recipients::institutions))
        .route("/webhooks/provider", post(webhooks::provider))
        .route("/webhooks/bitnob", post(webhooks::bitnob))
        .route("/webhooks/lnbits", post(webhooks::lnbits));

    let mut app = Router::new().nest("/api/v1", public.merge(protected));
    if state.config.dev_routes {
        // TODO: configure explicit origins once the frontend exists.
        app = app.layer(CorsLayer::permissive());
    }
    app.layer(TraceLayer::new_for_http()).with_state(state)
}

async fn health(State(state): State<AppState>) -> AppResult<Json<Value>> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "lightning": state.lightning.name(),
    })))
}

/// Bearer-token check for protected routes. Disabled when `API_KEY` is unset (local demo).
/// TODO: replace with per-user auth so /transactions can be scoped to the caller.
async fn require_api_key(State(state): State<AppState>, req: Request, next: Next) -> Result<Response, AppError> {
    let Some(expected) = state.config.api_key.as_deref() else {
        return Ok(next.run(req).await);
    };
    let provided = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => Ok(next.run(req).await),
        _ => Err(AppError::Unauthorized),
    }
}
