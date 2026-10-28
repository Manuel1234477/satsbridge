//! Refunds to the sender over Lightning, for payments in REFUND_PENDING (payout failed or was
//! reversed). The sender (or an admin) supplies a BOLT11 invoice for exactly the sats paid.
//!
//! `refund_payment_hash` is the lock: it is set before paying, cleared only when the payment
//! definitely failed, and kept while the outcome is unknown so a refund is never paid twice.

use chrono::Utc;

use crate::{
    adapters::{
        AdapterError,
        lightning::{InvoiceStatus, decode_invoice},
    },
    db::{self, PaymentIntent},
    domain::PaymentStatus,
    error::{AppError, AppResult},
    state::AppState,
};

fn max_fee_sats(amount_sats: i64) -> i64 {
    (amount_sats / 200).max(10)
}

pub async fn refund(
    state: &AppState,
    payment: &PaymentIntent,
    payment_request: &str,
    actor: &str,
) -> AppResult<PaymentIntent> {
    if payment.status != PaymentStatus::RefundPending {
        return Err(AppError::Conflict(format!(
            "payment is {}; nothing to refund",
            payment.status.as_str()
        )));
    }
    if payment.refund_payment_hash.is_some() {
        return Err(AppError::Conflict("a refund is already in progress".into()));
    }
    let invoice = decode_invoice(payment_request).map_err(|e| AppError::Validation(e.to_string()))?;
    let sats = payment.amount_sats;
    if invoice.amount_msat != Some(sats as u64 * 1000) {
        return Err(AppError::Validation(format!(
            "the refund invoice must be for exactly {sats} sats"
        )));
    }
    if invoice.expires_at_unix <= Utc::now().timestamp().max(0) as u64 {
        return Err(AppError::Validation("the refund invoice has expired".into()));
    }
    let id = payment.id.as_str();
    if !db::claim_refund(&state.pool, id, &invoice.payment_hash, sats).await? {
        return Err(AppError::Conflict("a refund is already in progress".into()));
    }
    db::insert_audit(
        &state.pool,
        id,
        "refund.started",
        None,
        None,
        Some(&format!("{actor}: {}", invoice.payment_hash)),
    )
    .await?;

    match state.lightning.pay_invoice(payment_request, max_fee_sats(sats)).await {
        Ok(sent) => complete(state, id, &sent.payment_hash).await?,
        Err(AdapterError::Unavailable(reason)) => {
            // Possibly in flight: keep the lock; the recovery worker checks the outcome.
            db::insert_audit(&state.pool, id, "refund.in_flight", None, None, Some(&reason)).await?;
        }
        Err(err) => {
            db::release_refund(&state.pool, id).await?;
            db::insert_audit(&state.pool, id, "refund.failed", None, None, Some(&err.to_string())).await?;
            return Err(err.into());
        }
    }
    db::get_payment(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("payment"))
}

async fn complete(state: &AppState, id: &str, payment_hash: &str) -> AppResult<()> {
    let mut tx = state.pool.begin().await?;
    if db::transition(
        &mut tx,
        id,
        PaymentStatus::RefundPending,
        PaymentStatus::Refunded,
        "refund.completed",
        Some(payment_hash),
    )
    .await?
    {
        db::mark_refunded(&mut *tx, id, Utc::now()).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Resolves refunds whose payment outcome was unknown. Returns how many completed.
pub async fn check_in_flight(state: &AppState) -> AppResult<usize> {
    let mut completed = 0;
    for payment in db::list_refunds_in_flight(&state.pool).await? {
        let Some(hash) = payment.refund_payment_hash.as_deref() else {
            continue;
        };
        match state.lightning.invoice_status(hash).await {
            Ok(InvoiceStatus::Paid { .. }) => {
                complete(state, &payment.id, hash).await?;
                completed += 1;
            }
            Ok(InvoiceStatus::Failed) => {
                db::release_refund(&state.pool, &payment.id).await?;
                db::insert_audit(
                    &state.pool,
                    &payment.id,
                    "refund.failed",
                    None,
                    None,
                    Some("Lightning payment failed"),
                )
                .await?;
            }
            Ok(InvoiceStatus::Pending) => {}
            Err(err) => tracing::warn!(payment_id = %payment.id, error = %err, "refund status check failed"),
        }
    }
    Ok(completed)
}
