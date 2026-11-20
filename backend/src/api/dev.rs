//! Demo-only helpers that stand in for the *mock* providers' callbacks: a bank transfer into a
//! mock collection account, and payout/refund results from mock payout rails. Mounted only when
//! `DEV_ROUTES=true`. Stellar settlement is never simulated here: it runs on whatever ledger is
//! configured (testnet, or the simulated ledger).

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::CurrentUser;
use crate::{
    adapters::collection::SenderAccount,
    db,
    error::{AppError, AppResult},
    services::{
        new_id,
        payments::{self, DepositEvent, PayoutEvent, PayoutEventStatus},
    },
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dev/payments/{id}/deposit", post(deposit))
        .route("/dev/payments/{id}/payout", post(payout))
        .route("/dev/payments/{id}/refund", post(refund))
}

#[derive(Deserialize, Default)]
struct SimulateDeposit {
    /// Defaults to the expected amount.
    amount_minor: Option<i64>,
    /// The account the transfer comes from. Defaults to a demo account; `null` hides it, as
    /// some providers do (refunds then need an account from the sender).
    #[serde(default = "demo_sender")]
    sender: Option<SenderAccount>,
}

fn demo_sender() -> Option<SenderAccount> {
    Some(SenderAccount {
        bank_code: "058".into(),
        account_number: "0123456789".into(),
        account_name: Some("DEMO SENDER".into()),
    })
}

async fn deposit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Option<Json<SimulateDeposit>>,
) -> AppResult<Json<Value>> {
    payments::owned_payment(&state, &id, &user.0).await?;
    let deposit = db::get_deposit(&state.pool, &id)
        .await?
        .ok_or_else(|| AppError::Conflict("call /pay first to get deposit instructions".into()))?;
    if !deposit.provider.ends_with("-mock") {
        return Err(AppError::Conflict(format!(
            "{} is a real provider; deposits come from its own webhooks",
            deposit.provider
        )));
    }
    let body = body.map(|Json(b)| b).unwrap_or(SimulateDeposit {
        amount_minor: None,
        sender: demo_sender(),
    });
    let event = DepositEvent {
        event_id: new_id("devdep"),
        reference: deposit.reference,
        amount_minor: body.amount_minor.unwrap_or(deposit.expected_amount),
        currency: deposit.currency,
        sender: body.sender,
    };
    let outcome = payments::handle_deposit(&state, event).await?;
    Ok(Json(json!({ "outcome": outcome })))
}

#[derive(Deserialize)]
struct SimulateResult {
    status: PayoutEventStatus,
    reason: Option<String>,
}

fn require_mock(provider: &str) -> AppResult<()> {
    if provider.ends_with("-mock") {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "{provider} is a real provider; its status comes from its own webhooks"
        )))
    }
}

async fn send(state: &AppState, reference: Option<String>, body: SimulateResult) -> AppResult<Json<Value>> {
    let provider_reference = reference.ok_or_else(|| AppError::Conflict("not submitted to a provider yet".into()))?;
    let event = PayoutEvent {
        event_id: new_id("devpo"),
        provider_reference,
        status: body.status,
        reason: body.reason,
    };
    let outcome = payments::handle_payout_event(state, event).await?;
    Ok(Json(json!({ "outcome": outcome })))
}

async fn payout(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(body): Json<SimulateResult>,
) -> AppResult<Json<Value>> {
    payments::owned_payment(&state, &id, &user.0).await?;
    let payout = db::get_payout(&state.pool, &id)
        .await?
        .ok_or(AppError::NotFound("payout"))?;
    require_mock(&payout.provider)?;
    send(&state, payout.provider_reference, body).await
}

async fn refund(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(body): Json<SimulateResult>,
) -> AppResult<Json<Value>> {
    payments::owned_payment(&state, &id, &user.0).await?;
    let refund = db::get_refund(&state.pool, &id)
        .await?
        .ok_or(AppError::NotFound("refund"))?;
    require_mock(&refund.provider)?;
    send(&state, refund.provider_reference, body).await
}
