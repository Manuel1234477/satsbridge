//! Demo-only helper that simulates callbacks from the *mock* payout providers (Tando, NGN mock).
//! Mounted only when `DEV_ROUTES=true`. There is deliberately no way to fake Lightning
//! settlement: payments settle only when LNbits reports them paid.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::CurrentUser;
use crate::{
    db,
    error::{AppError, AppResult},
    services::{
        new_id,
        payments::{self, PayoutEvent, PayoutEventStatus},
    },
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/dev/payments/{id}/payout", post(payout))
}

#[derive(Deserialize)]
struct SimulatePayout {
    status: PayoutEventStatus,
    reason: Option<String>,
}

async fn payout(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(body): Json<SimulatePayout>,
) -> AppResult<Json<Value>> {
    payments::owned_payment(&state, &id, &user.0).await?;
    let payout = db::get_payout(&state.pool, &id)
        .await?
        .ok_or(AppError::NotFound("payout"))?;
    if !payout.provider.ends_with("-mock") {
        return Err(AppError::Conflict(format!(
            "{} is a real provider; its status comes from its own webhooks",
            payout.provider
        )));
    }
    let provider_reference = payout
        .provider_reference
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
