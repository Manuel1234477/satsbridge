use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    db::{self, LightningPayment, PaymentIntent},
    error::AppResult,
    services::payments::{self, CreatePaymentRequest, PaymentDetails},
    state::AppState,
};

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreatePaymentRequest>,
) -> AppResult<(StatusCode, Json<PaymentIntent>)> {
    Ok((StatusCode::CREATED, Json(payments::create_payment(&state, req).await?)))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Json<PaymentDetails>> {
    Ok(Json(payments::get_details(&state, &id).await?))
}

pub async fn pay(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Json<LightningPayment>> {
    Ok(Json(payments::pay(&state, &id).await?))
}

#[derive(Deserialize)]
pub struct ListParams {
    limit: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> AppResult<Json<Vec<PaymentIntent>>> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    Ok(Json(db::list_payments(&state.pool, limit).await?))
}
