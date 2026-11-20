use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use super::CurrentUser;
use crate::{
    db::{self, Deposit, PaymentIntent, Refund},
    error::{AppError, AppResult},
    services::{
        payments::{self, CreatePaymentRequest, PaymentDetails},
        refunds::{self, RefundDestination},
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
) -> AppResult<Json<Deposit>> {
    let payment = payments::owned_payment(&state, &id, &user.0).await?;
    Ok(Json(payments::pay(&state, &payment).await?))
}

/// Refund to the account the deposit came from (empty body), or to the given NGN account.
pub async fn refund(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Option<Json<RefundDestination>>,
) -> AppResult<Json<Refund>> {
    let payment = payments::owned_payment(&state, &id, &user.0).await?;
    let refund = match body {
        Some(Json(destination)) => {
            refunds::start_to(&state, &payment, destination, &format!("user {}", user.0)).await?
        }
        None => refunds::start_automatic(&state, &payment).await?.ok_or_else(|| {
            AppError::Validation("the account this payment came from is unknown; give a bank account".into())
        })?,
    };
    Ok(Json(refund))
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
