//! Payment orchestration: payment intent -> Lightning invoice -> settlement -> local payout.
//!
//! Exactly-once payout relies on three guards: webhook event dedup (`processed_webhooks`),
//! compare-and-set status transitions, and the one-payout-per-payment primary key.

use anyhow::anyhow;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;

use super::{new_id, recipients};
use crate::{
    adapters::{
        AdapterError,
        payout::{PayoutAdapter, PayoutRequest},
    },
    db::{self, AuditEvent, LightningPayment, LightningStatus, PaymentIntent, Payout, PayoutStatus},
    domain::{PaymentStatus, Recipient},
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct CreatePaymentRequest {
    pub quote_id: String,
    pub recipient: Recipient,
}

/// Payment status plus everything needed for a receipt (FR-08, FR-11).
#[derive(Debug, Serialize)]
pub struct PaymentDetails {
    #[serde(flatten)]
    pub payment: PaymentIntent,
    pub lightning: Option<LightningPayment>,
    pub payout: Option<Payout>,
    pub events: Vec<AuditEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookOutcome {
    Processed,
    /// Same event ID seen before; nothing changed.
    Duplicate,
    /// Valid event that no longer applies to the payment's current state.
    Ignored,
}

#[derive(Debug, Deserialize)]
pub struct LightningEvent {
    pub event_id: String,
    pub payment_hash: String,
    pub amount_sats: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PayoutEventStatus {
    Success,
    Failed,
    Reversed,
}

#[derive(Debug, Deserialize)]
pub struct PayoutEvent {
    pub event_id: String,
    pub provider_reference: String,
    pub status: PayoutEventStatus,
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn create_payment(state: &AppState, req: CreatePaymentRequest) -> AppResult<PaymentIntent> {
    let quote = db::get_quote(&state.pool, &req.quote_id)
        .await?
        .ok_or(AppError::NotFound("quote"))?;
    if quote.expires_at <= Utc::now() {
        return Err(AppError::QuoteExpired);
    }
    let rail = req.recipient.rail();
    if rail != quote.destination_rail {
        return Err(AppError::Validation(format!(
            "quote is for {} but the recipient uses {}",
            quote.destination_rail.as_str(),
            rail.as_str()
        )));
    }
    let adapter = state.payouts.get(quote.destination_currency, rail).ok_or_else(|| {
        AppError::Validation(format!(
            "{} payouts to {} are not available",
            rail.as_str(),
            quote.destination_currency
        ))
    })?;
    let resolved = recipients::verify_for_payment(state, adapter.as_ref(), &req.recipient).await?;
    let payout_sats = quote.amount_sats - quote.fee_sats;
    if resolved.min_sats.is_some_and(|min| payout_sats < min) || resolved.max_sats.is_some_and(|max| payout_sats > max)
    {
        return Err(AppError::Validation(
            "amount is outside what this recipient can receive".into(),
        ));
    }
    let recipient_name = resolved.account_name;

    let now = Utc::now();
    let mut payment = PaymentIntent {
        id: new_id("pay"),
        quote_id: quote.id,
        source_currency: quote.source_currency,
        destination_currency: quote.destination_currency,
        source_amount: quote.source_amount,
        expected_payout: quote.destination_amount,
        amount_sats: quote.amount_sats,
        fee_sats: quote.fee_sats,
        exchange_rate: quote.exchange_rate,
        recipient_rail: rail,
        recipient_masked: req.recipient.masked_reference(),
        recipient: Json(req.recipient),
        recipient_name,
        status: PaymentStatus::Created,
        failure_reason: None,
        created_at: now,
        updated_at: now,
        expires_at: quote.expires_at,
    };

    let mut tx = state.pool.begin().await?;
    if let Err(err) = db::insert_payment(&mut *tx, &payment).await {
        return Err(if db::is_unique_violation(&err) {
            AppError::Conflict("this quote has already been used for a payment".into())
        } else {
            err.into()
        });
    }
    db::insert_audit(
        &mut *tx,
        &payment.id,
        "payment.created",
        None,
        Some(PaymentStatus::Created),
        None,
    )
    .await?;
    db::transition(
        &mut tx,
        &payment.id,
        PaymentStatus::Created,
        PaymentStatus::Quoted,
        "quote.locked",
        None,
    )
    .await?;
    tx.commit().await?;

    payment.status = PaymentStatus::Quoted;
    Ok(payment)
}

pub async fn get_details(state: &AppState, id: &str) -> AppResult<PaymentDetails> {
    let payment = db::get_payment(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("payment"))?;
    let (lightning, payout, events) = tokio::try_join!(
        db::get_lightning_by_payment(&state.pool, id),
        db::get_payout(&state.pool, id),
        db::list_audit(&state.pool, id),
    )?;
    Ok(PaymentDetails {
        payment,
        lightning,
        payout,
        events,
    })
}

/// Returns the Lightning invoice for a payment, creating it on first call. Safe to call repeatedly.
pub async fn pay(state: &AppState, id: &str) -> AppResult<LightningPayment> {
    let payment = db::get_payment(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("payment"))?;
    match payment.status {
        PaymentStatus::AwaitingPayment => return existing_invoice(state, id).await,
        PaymentStatus::Quoted => {}
        other => {
            return Err(AppError::Conflict(format!(
                "payment is {}; no invoice can be issued",
                other.as_str()
            )));
        }
    }

    if payment.expires_at <= Utc::now() {
        let mut conn = state.pool.acquire().await?;
        db::transition(
            &mut conn,
            id,
            PaymentStatus::Quoted,
            PaymentStatus::Expired,
            "quote.expired",
            None,
        )
        .await?;
        return Err(AppError::QuoteExpired);
    }

    let invoice = state
        .lightning
        .create_invoice(
            payment.amount_sats,
            &format!("SatsBridge {id}"),
            state.config.invoice_expiry_secs,
        )
        .await?;
    let lightning = LightningPayment {
        payment_intent_id: payment.id.clone(),
        payment_request: invoice.payment_request,
        payment_hash: invoice.payment_hash,
        amount_sats: payment.amount_sats,
        status: LightningStatus::Pending,
        expires_at: invoice.expires_at,
        settled_at: None,
        created_at: Utc::now(),
    };

    let mut tx = state.pool.begin().await?;
    if let Err(err) = db::insert_lightning(&mut *tx, &lightning).await {
        if db::is_unique_violation(&err) {
            // A concurrent call won the race; return its invoice.
            drop(tx);
            return existing_invoice(state, id).await;
        }
        return Err(err.into());
    }
    let moved = db::transition(
        &mut tx,
        id,
        PaymentStatus::Quoted,
        PaymentStatus::AwaitingPayment,
        "invoice.created",
        Some(&lightning.payment_hash),
    )
    .await?;
    if !moved {
        return Err(AppError::Conflict("payment changed state; retry".into()));
    }
    db::set_payment_expiry(&mut *tx, id, lightning.expires_at).await?;
    tx.commit().await?;
    Ok(lightning)
}

async fn existing_invoice(state: &AppState, id: &str) -> AppResult<LightningPayment> {
    db::get_lightning_by_payment(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow!("payment {id} is awaiting payment but has no invoice")))
}

/// Settlement -> exactly one payout request.
pub async fn handle_lightning_settled(state: &AppState, event: LightningEvent) -> AppResult<WebhookOutcome> {
    let mut tx = state.pool.begin().await?;
    if !db::record_webhook(&mut *tx, "lightning", &event.event_id).await? {
        return Ok(WebhookOutcome::Duplicate);
    }
    let lightning = db::get_lightning_by_hash(&mut *tx, &event.payment_hash)
        .await?
        .ok_or(AppError::NotFound("lightning payment"))?;
    if event.amount_sats < lightning.amount_sats {
        return Err(AppError::Validation(
            "settled amount is below the invoice amount".into(),
        ));
    }
    let payment = db::get_payment(&mut *tx, &lightning.payment_intent_id)
        .await?
        .ok_or(AppError::NotFound("payment"))?;
    let id = payment.id.as_str();

    let from = payment.status;
    let settled = matches!(from, PaymentStatus::AwaitingPayment | PaymentStatus::PaymentDetected)
        && db::transition(
            &mut tx,
            id,
            from,
            PaymentStatus::Settled,
            "lightning.settled",
            Some(&event.payment_hash),
        )
        .await?;
    if !settled {
        // Keep the dedup record so this event is not reprocessed.
        tx.commit().await?;
        return Ok(WebhookOutcome::Ignored);
    }
    db::mark_lightning_settled(&mut *tx, id, Utc::now()).await?;

    let adapter = state
        .payouts
        .get(payment.destination_currency, payment.recipient_rail)
        .ok_or_else(|| {
            AppError::Internal(anyhow!(
                "no {} payout adapter for {}",
                payment.recipient_rail.as_str(),
                payment.destination_currency
            ))
        })?;
    db::transition(
        &mut tx,
        id,
        PaymentStatus::Settled,
        PaymentStatus::PayoutPending,
        "payout.requested",
        Some(adapter.provider()),
    )
    .await?;
    db::insert_payout(
        &mut *tx,
        &Payout {
            payment_intent_id: payment.id.clone(),
            provider: adapter.provider().to_string(),
            rail: payment.recipient_rail,
            destination_currency: payment.destination_currency,
            destination_amount: payment.expected_payout,
            destination_reference: payment.recipient_masked.clone(),
            account_name: payment.recipient_name.clone(),
            provider_reference: None,
            status: PayoutStatus::Pending,
            failure_reason: None,
            created_at: Utc::now(),
            completed_at: None,
            reversed_at: None,
        },
    )
    .await?;
    tx.commit().await?;

    // TODO: move submission to an outbox worker so a crash here is retried automatically.
    // Retrying is safe because the payment ID is the provider idempotency key.
    submit_payout(state, &payment, adapter.as_ref()).await?;
    Ok(WebhookOutcome::Processed)
}

async fn submit_payout(state: &AppState, payment: &PaymentIntent, adapter: &dyn PayoutAdapter) -> AppResult<()> {
    let request = PayoutRequest {
        idempotency_key: &payment.id,
        recipient: &payment.recipient.0,
        account_name: payment.recipient_name.as_deref(),
        amount_minor: payment.expected_payout,
        amount_sats: payment.amount_sats - payment.fee_sats,
        currency: payment.destination_currency,
        narration: format!("SatsBridge {}", payment.id),
    };
    match adapter.create_payout(request).await {
        Ok(accepted) => {
            let reference = accepted.provider_reference.as_str();
            let mut tx = state.pool.begin().await?;
            db::mark_payout_submitted(&mut *tx, &payment.id, reference).await?;
            db::insert_audit(&mut *tx, &payment.id, "payout.submitted", None, None, Some(reference)).await?;
            if accepted.completed {
                // Rail confirmed delivery synchronously (Lightning Address); no webhook will follow.
                db::update_payout_status(&mut *tx, &payment.id, PayoutStatus::Completed, None, Utc::now()).await?;
                db::transition(
                    &mut tx,
                    &payment.id,
                    PaymentStatus::PayoutPending,
                    PaymentStatus::Completed,
                    "payout.completed",
                    Some(reference),
                )
                .await?;
            }
            tx.commit().await?;
        }
        Err(AdapterError::Unavailable(reason)) => {
            tracing::warn!(payment_id = %payment.id, %reason, "payout provider unavailable; will retry");
            db::insert_audit(
                &state.pool,
                &payment.id,
                "payout.submit_deferred",
                None,
                None,
                Some(&reason),
            )
            .await?;
        }
        Err(err) => {
            let reason = err.to_string();
            tracing::error!(payment_id = %payment.id, %reason, "payout rejected");
            let mut tx = state.pool.begin().await?;
            db::update_payout_status(&mut *tx, &payment.id, PayoutStatus::Failed, Some(&reason), Utc::now()).await?;
            db::set_failure_reason(&mut *tx, &payment.id, &reason).await?;
            db::transition(
                &mut tx,
                &payment.id,
                PaymentStatus::PayoutPending,
                PaymentStatus::Failed,
                "payout.rejected",
                Some(&reason),
            )
            .await?;
            tx.commit().await?;
        }
    }
    Ok(())
}

pub async fn handle_payout_event(state: &AppState, event: PayoutEvent) -> AppResult<WebhookOutcome> {
    let mut tx = state.pool.begin().await?;
    let payout = db::get_payout_by_reference(&mut *tx, &event.provider_reference)
        .await?
        .ok_or(AppError::NotFound("payout"))?;
    if !db::record_webhook(&mut *tx, &format!("provider:{}", payout.provider), &event.event_id).await? {
        return Ok(WebhookOutcome::Duplicate);
    }
    let id = payout.payment_intent_id.as_str();
    let now = Utc::now();
    let reference = Some(event.provider_reference.as_str());

    let applied = match event.status {
        PayoutEventStatus::Success => {
            let ok = db::transition(
                &mut tx,
                id,
                PaymentStatus::PayoutPending,
                PaymentStatus::Completed,
                "payout.completed",
                reference,
            )
            .await?;
            if ok {
                db::update_payout_status(&mut *tx, id, PayoutStatus::Completed, None, now).await?;
            }
            ok
        }
        PayoutEventStatus::Failed => {
            let reason = event.reason.as_deref().unwrap_or("payout failed at provider");
            let ok = db::transition(
                &mut tx,
                id,
                PaymentStatus::PayoutPending,
                PaymentStatus::Failed,
                "payout.failed",
                Some(reason),
            )
            .await?;
            if ok {
                db::update_payout_status(&mut *tx, id, PayoutStatus::Failed, Some(reason), now).await?;
                db::set_failure_reason(&mut *tx, id, reason).await?;
            }
            ok
        }
        PayoutEventStatus::Reversed => {
            // NGN addendum A.6: a reversal after success re-opens the payment for refund.
            let reason = event.reason.as_deref().unwrap_or("payout reversed by provider");
            let ok = db::transition(
                &mut tx,
                id,
                PaymentStatus::Completed,
                PaymentStatus::Failed,
                "payout.reversed",
                Some(reason),
            )
            .await?;
            if ok {
                db::transition(
                    &mut tx,
                    id,
                    PaymentStatus::Failed,
                    PaymentStatus::RefundPending,
                    "refund.pending",
                    None,
                )
                .await?;
                db::update_payout_status(&mut *tx, id, PayoutStatus::Reversed, Some(reason), now).await?;
                db::set_failure_reason(&mut *tx, id, reason).await?;
            }
            ok
        }
    };
    tx.commit().await?;
    Ok(if applied {
        WebhookOutcome::Processed
    } else {
        WebhookOutcome::Ignored
    })
}
