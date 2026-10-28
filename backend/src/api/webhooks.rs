use axum::{Json, body::Bytes, extract::State, http::HeaderMap};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{
    adapters::{bitnob, lightning::lnbits},
    error::{AppError, AppResult},
    security::{SIGNATURE_HEADER, verify_hmac_sha512, verify_signature},
    services::{
        payments::{self, WebhookOutcome},
        settlement,
    },
    state::AppState,
};

fn verified_body<T: DeserializeOwned>(secret: &str, headers: &HeaderMap, body: &Bytes) -> AppResult<T> {
    let signature = headers.get(SIGNATURE_HEADER).and_then(|v| v.to_str().ok());
    if !verify_signature(secret, body, signature) {
        return Err(AppError::InvalidSignature);
    }
    serde_json::from_slice(body).map_err(|e| AppError::Validation(format!("invalid webhook body: {e}")))
}

fn ack(outcome: WebhookOutcome) -> Json<Value> {
    Json(json!({ "outcome": outcome }))
}

pub async fn provider(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> AppResult<Json<Value>> {
    let event = verified_body(&state.config.provider_webhook_secret, &headers, &body)?;
    Ok(ack(payments::handle_payout_event(&state, event).await?))
}

/// Bitnob payout callbacks: HMAC-SHA512 of the raw body in `x-bitnob-signature`.
/// Non-2xx responses are retried by Bitnob, which covers events that arrive before we have
/// stored the payout reference.
pub async fn bitnob(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> AppResult<Json<Value>> {
    let Some(secret) = state.config.payouts.bitnob.as_ref().map(|b| b.webhook_secret.as_str()) else {
        return Err(AppError::NotFound("bitnob webhook"));
    };
    let signature = headers.get(bitnob::SIGNATURE_HEADER).and_then(|v| v.to_str().ok());
    if !verify_hmac_sha512(secret, &body, signature) {
        return Err(AppError::InvalidSignature);
    }
    let event: bitnob::WebhookEvent =
        serde_json::from_slice(&body).map_err(|e| AppError::Validation(format!("invalid Bitnob webhook body: {e}")))?;
    tracing::info!(event = %event.event, event_id = %event.event_id, payout_id = %event.data.id, "Bitnob webhook");
    match bitnob::to_payout_event(event) {
        Some(event) => Ok(ack(payments::handle_payout_event(&state, event).await?)),
        None => Ok(ack(WebhookOutcome::Ignored)),
    }
}

/// LNbits invoice webhook. Unsigned, so the body only tells us which invoice to re-check;
/// settlement is confirmed with LNbits before anything changes.
pub async fn lnbits(State(state): State<AppState>, body: Bytes) -> AppResult<Json<Value>> {
    let payment_hash = lnbits::webhook_payment_hash(&body)
        .ok_or_else(|| AppError::Validation("webhook body has no valid payment_hash".into()))?;
    Ok(ack(settlement::confirm_settlement(&state, &payment_hash).await?))
}
