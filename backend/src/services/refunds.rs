//! Refunds of the sender's deposit, in their own currency, for payments in REFUND_PENDING
//! (underpaid or late deposit, failed settlement, failed or reversed payout). They go out on the
//! source currency's payout rail: NGN bank transfer, M-Pesa, or mobile money.
//!
//! The refund goes back to the account the deposit came from when the collection provider
//! reported it (automatically), or to an account the sender (or an admin) gives. The full amount
//! received is returned. One refund row per payment: a failed refund can be retried to another
//! account, but a pending, submitted or completed one can't be started again.

use chrono::Utc;
use sqlx::types::Json;

use super::{
    payments::{PayoutEvent, PayoutEventStatus, WebhookOutcome},
    recipients,
};
use crate::{
    adapters::{
        AdapterError,
        payout::{PayoutAdapter, PayoutRequest},
    },
    db::{self, PaymentIntent, Refund, RefundStatus},
    domain::{PaymentStatus, Recipient},
    error::{AppError, AppResult},
    state::AppState,
};

fn idempotency_key(payment_id: &str) -> String {
    format!("{payment_id}-refund")
}

/// The source currency's payout rail for this destination.
fn adapter_for<'a>(
    state: &'a AppState,
    payment: &PaymentIntent,
    destination: &Recipient,
) -> AppResult<&'a std::sync::Arc<dyn PayoutAdapter>> {
    state
        .payouts
        .get(payment.source_currency, destination.rail())
        .ok_or_else(|| {
            AppError::Validation(format!(
                "refunds in {} can't go to a {} account",
                payment.source_currency,
                destination.rail().as_str()
            ))
        })
}

/// Starts the refund to the deposit's source account, if the provider reported one.
pub async fn start_automatic(state: &AppState, payment: &PaymentIntent) -> AppResult<Option<Refund>> {
    let Some(deposit) = db::get_deposit(&state.pool, &payment.id).await? else {
        return Ok(None);
    };
    let Some(sender) = deposit.sender_account else {
        return Ok(None);
    };
    let events = db::list_audit(&state.pool, &payment.id).await?;
    if events.iter().any(|e| e.event_type == "refund.source_unusable") {
        return Ok(None); // already tried; waiting for the sender to give an account
    }
    let recipient = sender.0;
    let resolved = match adapter_for(state, payment, &recipient) {
        Ok(adapter) => adapter.resolve_recipient(&recipient).await,
        Err(err) => Err(AdapterError::InvalidRecipient(err.to_string())),
    };
    let account_name = match resolved {
        Ok(resolved) => resolved.account_name,
        Err(err @ (AdapterError::InvalidRecipient(_) | AdapterError::RecipientNotFound)) => {
            // The sender has to give another account.
            db::insert_audit(
                &state.pool,
                &payment.id,
                "refund.source_unusable",
                None,
                None,
                Some(&err.to_string()),
            )
            .await?;
            return Ok(None);
        }
        Err(err) => return Err(err.into()),
    };
    start(state, payment, recipient, account_name, "automatic")
        .await
        .map(Some)
}

/// Starts a refund to an account the sender or an admin chose, in the source currency: an NGN
/// bank account (with a `resolution_id`), an M-Pesa number, or a mobile money wallet.
pub async fn start_to(
    state: &AppState,
    payment: &PaymentIntent,
    recipient: Recipient,
    actor: &str,
) -> AppResult<Refund> {
    let adapter = adapter_for(state, payment, &recipient)?;
    let resolved = recipients::verify_for_payment(state, adapter.as_ref(), &recipient).await?;
    start(state, payment, recipient, resolved.account_name, actor).await
}

async fn start(
    state: &AppState,
    payment: &PaymentIntent,
    recipient: Recipient,
    account_name: Option<String>,
    actor: &str,
) -> AppResult<Refund> {
    if payment.status != PaymentStatus::RefundPending {
        return Err(AppError::Conflict(format!(
            "payment is {}; nothing to refund",
            payment.status.as_str()
        )));
    }
    let deposit = db::get_deposit(&state.pool, &payment.id)
        .await?
        .ok_or_else(|| AppError::Conflict("no deposit was received for this payment".into()))?;
    let amount = deposit
        .received_amount
        .ok_or_else(|| AppError::Conflict("no deposit was received for this payment".into()))?;
    let adapter = adapter_for(state, payment, &recipient)?;
    let now = Utc::now();
    let refund = Refund {
        payment_intent_id: payment.id.clone(),
        provider: adapter.provider().to_string(),
        currency: payment.source_currency,
        amount,
        destination_reference: recipient.masked_reference(),
        destination: Json(recipient),
        account_name,
        provider_reference: None,
        status: RefundStatus::Pending,
        failure_reason: None,
        attempts: 0,
        created_at: now,
        updated_at: now,
        completed_at: None,
    };
    if !db::upsert_refund(&state.pool, &refund).await? {
        return Err(AppError::Conflict("a refund is already in progress".into()));
    }
    let current = db::get_refund(&state.pool, &payment.id)
        .await?
        .ok_or(AppError::NotFound("refund"))?;
    db::insert_audit(
        &state.pool,
        &payment.id,
        "refund.started",
        None,
        None,
        Some(&format!(
            "{actor}: {} {} to {}",
            amount, refund.currency, refund.destination_reference
        )),
    )
    .await?;
    submit(state, &current, adapter.as_ref()).await?;
    db::get_refund(&state.pool, &payment.id)
        .await?
        .ok_or(AppError::NotFound("refund"))
}

/// Submits the next attempt of a pending refund, if this caller wins the claim.
pub async fn submit(state: &AppState, refund: &Refund, adapter: &dyn PayoutAdapter) -> AppResult<bool> {
    let id = refund.payment_intent_id.as_str();
    if !db::claim_refund_attempt(&state.pool, id, refund.attempts).await? {
        return Ok(false);
    }
    let key = idempotency_key(id);
    let request = PayoutRequest {
        idempotency_key: &key,
        attempt: refund.attempts + 1,
        recipient: &refund.destination.0,
        account_name: refund.account_name.as_deref(),
        amount_minor: refund.amount,
        currency: refund.currency,
        narration: format!("StellarBridge refund {id}"),
    };
    match adapter.create_payout(request).await {
        Ok(accepted) => {
            db::update_refund(
                &state.pool,
                id,
                RefundStatus::Submitted,
                Some(&accepted.provider_reference),
                None,
            )
            .await?;
            db::insert_audit(
                &state.pool,
                id,
                "refund.submitted",
                None,
                None,
                Some(&accepted.provider_reference),
            )
            .await?;
            if accepted.completed {
                complete(state, id, &accepted.provider_reference).await?;
            }
        }
        Err(AdapterError::Unavailable(reason)) => {
            db::insert_audit(&state.pool, id, "refund.attempt_unconfirmed", None, None, Some(&reason)).await?;
        }
        Err(err) => {
            let reason = err.to_string();
            db::update_refund(&state.pool, id, RefundStatus::Failed, None, Some(&reason)).await?;
            db::insert_audit(&state.pool, id, "refund.failed", None, None, Some(&reason)).await?;
        }
    }
    Ok(true)
}

async fn complete(state: &AppState, id: &str, reference: &str) -> AppResult<bool> {
    let mut tx = state.pool.begin().await?;
    let moved = db::transition(
        &mut tx,
        id,
        PaymentStatus::RefundPending,
        PaymentStatus::Refunded,
        "refund.completed",
        Some(reference),
    )
    .await?;
    if moved {
        db::update_refund(&mut *tx, id, RefundStatus::Completed, None, None).await?;
    }
    tx.commit().await?;
    Ok(moved)
}

/// Provider callback for a refund payout.
pub async fn handle_event(state: &AppState, event: PayoutEvent) -> AppResult<WebhookOutcome> {
    let refund = db::get_refund_by_reference(&state.pool, &event.provider_reference)
        .await?
        .ok_or(AppError::NotFound("payout"))?;
    if !db::record_webhook(&state.pool, &format!("provider:{}", refund.provider), &event.event_id).await? {
        return Ok(WebhookOutcome::Duplicate);
    }
    let id = refund.payment_intent_id.as_str();
    let applied = match event.status {
        PayoutEventStatus::Success => complete(state, id, &event.provider_reference).await?,
        PayoutEventStatus::Failed | PayoutEventStatus::Reversed if refund.status == RefundStatus::Submitted => {
            let reason = event.reason.as_deref().unwrap_or("refund failed at provider");
            db::update_refund(&state.pool, id, RefundStatus::Failed, None, Some(reason)).await?;
            db::insert_audit(&state.pool, id, "refund.failed", None, None, Some(reason)).await?;
            true
        }
        _ => false,
    };
    Ok(if applied {
        WebhookOutcome::Processed
    } else {
        WebhookOutcome::Ignored
    })
}

/// Recovery for refunds: resubmits unconfirmed attempts where that is safe, and asks the
/// provider about submitted ones with no callback. Returns how many completed.
pub async fn recover(state: &AppState) -> AppResult<usize> {
    let cfg = state.config.recovery;
    let now = Utc::now();
    let mut completed = 0;

    let retry_before = now - chrono::Duration::seconds(cfg.base_backoff_secs);
    for refund in db::list_refunds(&state.pool, RefundStatus::Pending, retry_before).await? {
        let Some(payment) = db::get_payment(&state.pool, &refund.payment_intent_id).await? else {
            continue;
        };
        let Ok(adapter) = adapter_for(state, &payment, &refund.destination.0) else {
            continue;
        };
        if refund.attempts >= cfg.max_attempts || (refund.attempts > 0 && !adapter.safe_to_retry()) {
            continue; // left for an admin; visible on the payment
        }
        if refund.attempts > 0 {
            match adapter.find_payout(&idempotency_key(&refund.payment_intent_id)).await {
                Ok(Some(accepted)) => {
                    db::update_refund(
                        &state.pool,
                        &refund.payment_intent_id,
                        RefundStatus::Submitted,
                        Some(&accepted.provider_reference),
                        None,
                    )
                    .await?;
                    continue;
                }
                Ok(None) => {}
                Err(_) => continue,
            }
        }
        submit(state, &refund, adapter.as_ref()).await?;
    }

    let confirm_before = now - chrono::Duration::seconds(cfg.unconfirmed_after_secs);
    for refund in db::list_refunds(&state.pool, RefundStatus::Submitted, confirm_before).await? {
        let (Some(reference), Some(payment)) = (
            refund.provider_reference.as_deref(),
            db::get_payment(&state.pool, &refund.payment_intent_id).await?,
        ) else {
            continue;
        };
        let Ok(adapter) = adapter_for(state, &payment, &refund.destination.0) else {
            continue;
        };
        if let Ok(Some(status)) = adapter.payout_status(reference).await {
            let event = PayoutEvent {
                event_id: format!("status:{reference}:{status:?}"),
                provider_reference: reference.to_string(),
                status,
                reason: Some("status checked with provider".into()),
            };
            if matches!(handle_event(state, event).await?, WebhookOutcome::Processed)
                && matches!(status, PayoutEventStatus::Success)
            {
                completed += 1;
            }
        }
    }
    Ok(completed)
}
