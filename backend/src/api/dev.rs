//! Demo-only helpers that drive the mock adapters through the same code paths as real webhooks.
//! Mounted only when `DEV_ROUTES=true`.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    db,
    error::{AppError, AppResult},
    services::{
        new_id,
        payments::{self, LightningEvent, PayoutEvent, PayoutEventStatus},
    },
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dev/payments/{id}/settle", post(settle))
        .route("/dev/payments/{id}/payout", post(payout))
}

async fn settle(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Json<Value>> {
    let lightning = db::get_lightning_by_payment(&state.pool, &id)
        .await?
        .ok_or(AppError::NotFound("lightning payment"))?;
    let event = LightningEvent {
        event_id: new_id("devln"),
        payment_hash: lightning.payment_hash,
        amount_sats: lightning.amount_sats,
    };
    let outcome = payments::handle_lightning_settled(&state, event).await?;
    Ok(Json(json!({ "outcome": outcome })))
}

#[derive(Deserialize)]
struct SimulatePayout {
    status: PayoutEventStatus,
    reason: Option<String>,
}

async fn payout(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SimulatePayout>,
) -> AppResult<Json<Value>> {
    let provider_reference = db::get_payout(&state.pool, &id)
        .await?
        .and_then(|p| p.provider_reference)
        .ok_or_else(|| AppError::Conflict("payout has not been submitted to a provider".into()))?;
    let event = PayoutEvent {
        event_id: new_id("devpo"),
        provider_reference,
        status: body.status,
        reason: body.reason,
    };
    let outcome = payments::handle_payout_event(&state, event).await?;
    Ok(Json(json!({ "outcome": outcome })))
}
