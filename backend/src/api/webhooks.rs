use axum::{Json, body::Bytes, extract::State, http::HeaderMap};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{
    error::{AppError, AppResult},
    security::{SIGNATURE_HEADER, verify_signature},
    services::payments::{self, WebhookOutcome},
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

/// Bank-transfer deposits from the collection provider, signed like `/webhooks/provider`.
pub async fn collections(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> AppResult<Json<Value>> {
    let event = verified_body(&state.config.provider_webhook_secret, &headers, &body)?;
    Ok(ack(payments::handle_deposit(&state, event).await?))
}
