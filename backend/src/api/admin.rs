//! `/api/v1/admin/*`, behind `ADMIN_API_KEY`.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    db::PaymentIntent,
    domain::PaymentStatus,
    error::AppResult,
    services::{
        admin::{self, AdminPayment, Summary},
        payments::{self, PaymentDetails},
        recovery::{self, RecoveryReport},
        refunds,
    },
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/summary", get(summary))
        .route("/admin/payments", get(list))
        .route("/admin/payments/{id}", get(details))
        .route("/admin/payments/{id}/retry-payout", post(retry_payout))
        .route("/admin/payments/{id}/fail-payout", post(fail_payout))
        .route("/admin/payments/{id}/refund", post(refund))
        .route("/admin/recovery/run", post(run_recovery))
}

async fn summary(State(state): State<AppState>) -> AppResult<Json<Summary>> {
    Ok(Json(admin::summary(&state).await?))
}

#[derive(Deserialize)]
struct ListParams {
    status: Option<PaymentStatus>,
    #[serde(default)]
    needs_review: bool,
    limit: Option<i64>,
}

async fn list(State(state): State<AppState>, Query(p): Query<ListParams>) -> AppResult<Json<Vec<AdminPayment>>> {
    let limit = p.limit.unwrap_or(100).clamp(1, 500);
    Ok(Json(admin::list(&state, p.status, p.needs_review, limit).await?))
}

async fn details(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Json<PaymentDetails>> {
    let payment = admin::get(&state, &id).await?;
    Ok(Json(payments::get_details(&state, payment).await?))
}

#[derive(Deserialize, Default)]
struct RetryRequest {
    /// Retry even on rails that can't be retried automatically. Only after checking with the
    /// provider that the earlier attempt didn't pay.
    #[serde(default)]
    force: bool,
}

async fn retry_payout(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<RetryRequest>>,
) -> AppResult<Json<Value>> {
    let force = body.map(|Json(b)| b.force).unwrap_or_default();
    Ok(Json(
        json!({ "outcome": admin::retry_payout(&state, &id, force).await? }),
    ))
}

#[derive(Deserialize)]
struct FailRequest {
    reason: String,
}

async fn fail_payout(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<FailRequest>,
) -> AppResult<Json<PaymentIntent>> {
    admin::fail_payout(&state, &id, &req.reason).await?;
    Ok(Json(admin::get(&state, &id).await?))
}

#[derive(Deserialize)]
struct RefundRequest {
    payment_request: String,
}

async fn refund(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RefundRequest>,
) -> AppResult<Json<PaymentIntent>> {
    let payment = admin::get(&state, &id).await?;
    Ok(Json(
        refunds::refund(&state, &payment, &req.payment_request, "admin").await?,
    ))
}

async fn run_recovery(State(state): State<AppState>) -> AppResult<Json<RecoveryReport>> {
    Ok(Json(recovery::run_once(&state).await?))
}
