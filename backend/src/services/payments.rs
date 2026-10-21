//! Payment orchestration: payment intent -> Lightning invoice -> settlement -> local payout.
//!
//! Exactly-once payout relies on four guards: webhook event dedup (`processed_webhooks`),
//! compare-and-set status transitions, the one-payout-per-payment primary key, and atomically
//! claimed submission attempts (`db::claim_payout_attempt`).

use anyhow::anyhow;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;

use super::{lnurl, new_id, recipients};
use crate::{
    adapters::{
        AdapterError,
        lightning::InvoiceDescription,
        payout::{PayoutAccepted, PayoutAdapter, PayoutRequest},
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
    /// LNURL-pay code for this payment (`LNURL1...`), when `PUBLIC_BASE_URL` is set.
    pub lnurl: Option<String>,
    pub payout: Option<Payout>,
    pub events: Vec<AuditEvent>,
}

/// `POST /payments/{id}/pay` response: the invoice, plus the same payment as LNURL-pay.
#[derive(Debug, Serialize)]
pub struct PayResponse {
    #[serde(flatten)]
    pub lightning: LightningPayment,
    pub lnurl: Option<String>,
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

/// The payment, if it exists and belongs to `user_id`. Other users' payments are "not found".
pub async fn owned_payment(state: &AppState, id: &str, user_id: &str) -> AppResult<PaymentIntent> {
    db::get_payment(&state.pool, id)
        .await?
        .filter(|p| p.user_id.as_deref() == Some(user_id))
        .ok_or(AppError::NotFound("payment"))
}

pub async fn create_payment(state: &AppState, user_id: &str, req: CreatePaymentRequest) -> AppResult<PaymentIntent> {
    let quote = db::get_quote(&state.pool, &req.quote_id)
        .await?
        .filter(|q| q.user_id.as_deref() == Some(user_id))
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
    if resolved.min_minor.is_some_and(|min| quote.destination_amount < min)
        || resolved.max_minor.is_some_and(|max| quote.destination_amount > max)
    {
        return Err(AppError::Validation(format!(
            "amount is outside the payout limits for this recipient's country ({}..{} minor units)",
            resolved.min_minor.unwrap_or(0),
            resolved
                .max_minor
                .map(|m| m.to_string())
                .unwrap_or_else(|| "no max".into())
        )));
    }

    let now = Utc::now();
    let mut payment = PaymentIntent {
        id: new_id("pay"),
        user_id: Some(user_id.to_string()),
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
        recipient_name: resolved.account_name,
        status: PaymentStatus::Created,
        failure_reason: None,
        created_at: now,
        updated_at: now,
        expires_at: quote.expires_at,
        refund_payment_hash: None,
        refund_sats: None,
        refunded_at: None,
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

pub async fn get_details(state: &AppState, payment: PaymentIntent) -> AppResult<PaymentDetails> {
    let id = payment.id.as_str();
    let (lightning, payout, events) = tokio::try_join!(
        db::get_lightning_by_payment(&state.pool, id),
        db::get_payout(&state.pool, id),
        db::list_audit(&state.pool, id),
    )?;
    let lnurl = lnurl::for_payment(state, &payment);
    Ok(PaymentDetails {
        payment,
        lightning,
        lnurl,
        payout,
        events,
    })
}

/// Returns the Lightning invoice for a payment, creating it on first call. Safe to call repeatedly.
pub async fn pay(state: &AppState, payment: &PaymentIntent) -> AppResult<PayResponse> {
    let lightning = invoice_for(state, payment).await?;
    Ok(PayResponse {
        lightning,
        lnurl: lnurl::for_payment(state, payment),
    })
}

/// The payment's single invoice. It commits to the LNURL metadata, so wallets paying by BOLT11
/// and by LNURL-pay pay the same invoice and the sender can never pay twice.
pub async fn invoice_for(state: &AppState, payment: &PaymentIntent) -> AppResult<LightningPayment> {
    let id = payment.id.as_str();
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

    let description = InvoiceDescription {
        memo: format!("SatsBridge {id}"),
        lnurl_metadata: Some(lnurl::metadata(payment)),
    };
    let invoice = state
        .lightning
        .create_invoice(payment.amount_sats, &description, state.config.invoice_expiry_secs)
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

pub fn adapter_for<'a>(
    state: &'a AppState,
    payment: &PaymentIntent,
) -> AppResult<&'a std::sync::Arc<dyn PayoutAdapter>> {
    state
        .payouts
        .get(payment.destination_currency, payment.recipient_rail)
        .ok_or_else(|| {
            AppError::Internal(anyhow!(
                "no {} payout adapter for {}",
                payment.recipient_rail.as_str(),
                payment.destination_currency
            ))
        })
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

    let adapter = adapter_for(state, &payment)?;
    db::transition(
        &mut tx,
        id,
        PaymentStatus::Settled,
        PaymentStatus::PayoutPending,
        "payout.requested",
        Some(adapter.provider()),
    )
    .await?;
    let now = Utc::now();
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
            created_at: now,
            completed_at: None,
            reversed_at: None,
            attempts: 0,
            last_attempt_at: Some(now),
            needs_review: false,
            review_reason: None,
        },
    )
    .await?;
    tx.commit().await?;

    // If this process dies here, the recovery worker resubmits (see `services::recovery`).
    submit_payout(state, &payment, adapter.as_ref(), 0).await?;
    Ok(WebhookOutcome::Processed)
}

/// Submits attempt `previous_attempts + 1`, if this caller wins the claim for it.
/// Returns false when another caller already took that attempt.
pub async fn submit_payout(
    state: &AppState,
    payment: &PaymentIntent,
    adapter: &dyn PayoutAdapter,
    previous_attempts: i64,
) -> AppResult<bool> {
    {
        let mut conn = state.pool.acquire().await?;
        if !db::claim_payout_attempt(&mut conn, &payment.id, previous_attempts).await? {
            return Ok(false);
        }
    }
    let attempt = previous_attempts + 1;
    let request = PayoutRequest {
        idempotency_key: &payment.id,
        attempt,
        recipient: &payment.recipient.0,
        account_name: payment.recipient_name.as_deref(),
        amount_minor: payment.expected_payout,
        amount_sats: payment.amount_sats - payment.fee_sats,
        currency: payment.destination_currency,
        narration: format!("SatsBridge {}", payment.id),
    };
    match adapter.create_payout(request).await {
        Ok(accepted) => record_accepted(state, &payment.id, &accepted).await?,
        Err(AdapterError::Unavailable(reason)) => {
            tracing::warn!(payment_id = %payment.id, attempt, %reason, "payout attempt unconfirmed; recovery will follow up");
            db::insert_audit(
                &state.pool,
                &payment.id,
                "payout.attempt_unconfirmed",
                None,
                None,
                Some(&reason),
            )
            .await?;
            if !adapter.safe_to_retry() {
                let why = format!("{} payout outcome unknown: {reason}", adapter.provider());
                db::flag_payout_review(&state.pool, &payment.id, &why).await?;
            }
        }
        Err(err) => fail_payout(state, &payment.id, "payout.rejected", &err.to_string()).await?,
    }
    Ok(true)
}

/// Stores the provider's acceptance; completes the payment if the rail confirmed delivery.
pub async fn record_accepted(state: &AppState, payment_id: &str, accepted: &PayoutAccepted) -> AppResult<()> {
    let reference = accepted.provider_reference.as_str();
    let mut tx = state.pool.begin().await?;
    db::mark_payout_submitted(&mut *tx, payment_id, reference).await?;
    db::insert_audit(&mut *tx, payment_id, "payout.submitted", None, None, Some(reference)).await?;
    if accepted.completed {
        // Rail confirmed delivery synchronously (Lightning Address); no webhook will follow.
        db::update_payout_status(&mut *tx, payment_id, PayoutStatus::Completed, None, Utc::now()).await?;
        db::transition(
            &mut tx,
            payment_id,
            PaymentStatus::PayoutPending,
            PaymentStatus::Completed,
            "payout.completed",
            Some(reference),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// PAYOUT_PENDING -> FAILED -> REFUND_PENDING: the sender's sats are owed back.
async fn fail_payout(state: &AppState, payment_id: &str, event: &str, reason: &str) -> AppResult<()> {
    tracing::error!(%payment_id, %reason, "payout failed");
    let mut tx = state.pool.begin().await?;
    let failed = db::transition(
        &mut tx,
        payment_id,
        PaymentStatus::PayoutPending,
        PaymentStatus::Failed,
        event,
        Some(reason),
    )
    .await?;
    if failed {
        db::update_payout_status(&mut *tx, payment_id, PayoutStatus::Failed, Some(reason), Utc::now()).await?;
        db::set_failure_reason(&mut *tx, payment_id, reason).await?;
        db::transition(
            &mut tx,
            payment_id,
            PaymentStatus::Failed,
            PaymentStatus::RefundPending,
            "refund.pending",
            None,
        )
        .await?;
    }
    tx.commit().await?;
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
                db::transition(
                    &mut tx,
                    id,
                    PaymentStatus::Failed,
                    PaymentStatus::RefundPending,
                    "refund.pending",
                    None,
                )
                .await?;
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
