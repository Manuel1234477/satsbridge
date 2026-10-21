use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use super::CurrentUser;
use crate::{
    db::{self, PaymentIntent},
    error::AppResult,
    services::{
        payments::{self, CreatePaymentRequest, PayResponse, PaymentDetails},
        refunds,
    },
    state::AppState,
};

pub async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(req): Json<CreatePaymentRequest>,
) -> AppResult<(StatusCode, Json<PaymentIntent>)> {
    Ok((
        StatusCode::CREATED,
        Json(payments::create_payment(&state, &user.0, req).await?),
    ))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> AppResult<Json<PaymentDetails>> {
    let payment = payments::owned_payment(&state, &id, &user.0).await?;
    Ok(Json(payments::get_details(&state, payment).await?))
}

pub async fn pay(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> AppResult<Json<PayResponse>> {
    let payment = payments::owned_payment(&state, &id, &user.0).await?;
    Ok(Json(payments::pay(&state, &payment).await?))
}

#[derive(Deserialize)]
pub struct RefundRequest {
    /// BOLT11 invoice for exactly the sats the sender paid.
    pub payment_request: String,
}

pub async fn refund(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(req): Json<RefundRequest>,
) -> AppResult<Json<PaymentIntent>> {
    let payment = payments::owned_payment(&state, &id, &user.0).await?;
    Ok(Json(
        refunds::refund(&state, &payment, &req.payment_request, &format!("user {}", user.0)).await?,
    ))
}

#[derive(Deserialize)]
pub struct ListParams {
    limit: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> AppResult<Json<Vec<PaymentIntent>>> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    Ok(Json(db::list_payments_for_user(&state.pool, &user.0, limit).await?))
}
