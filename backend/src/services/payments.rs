//! Payment orchestration:
//!
//! ```text
//! quote -> payment -> bank deposit instructions -> deposit received
//!       -> Stellar settle (mint + path payment) -> local payout -> completed -> Stellar redeem
//!                                                 \-> failed -> Stellar reverse + refund
//! ```
//!
//! [`advance`] is the single, idempotent driver: given a payment's state it takes the next
//! step. Webhooks call it after recording an event and the recovery worker calls it for anything
//! left unfinished, so a crash at any point is picked up where it stopped.
//!
//! Exactly-once payout relies on four guards: webhook event dedup (`processed_webhooks`),
//! compare-and-set status transitions, the one-payout-per-payment primary key, and atomically
//! claimed submission attempts (`db::claim_payout_attempt`). Exactly-once Stellar settlement is
//! covered in [`super::ledger`].

use anyhow::anyhow;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;

use super::{ledger, new_id, recipients, refunds};
use crate::{
    adapters::{
        AdapterError,
        collection::{DepositRequest, SenderAccount},
        payout::{PayoutAccepted, PayoutAdapter, PayoutRequest},
        stellar::{LedgerOp, pricing::minor_to_stroops},
    },
    db::{
        self, AuditEvent, Deposit, DepositStatus, PaymentIntent, Payout, PayoutStatus, Refund, StellarTx,
        StellarTxKind, StellarTxStatus,
    },
    domain::{PaymentStatus, Recipient},
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct CreatePaymentRequest {
    pub quote_id: String,
    pub recipient: Recipient,
}

#[derive(Debug, Serialize)]
pub struct StellarTxView {
    #[serde(flatten)]
    pub tx: StellarTx,
    pub explorer_url: Option<String>,
}

/// Payment status plus everything needed for a receipt.
#[derive(Debug, Serialize)]
pub struct PaymentDetails {
    #[serde(flatten)]
    pub payment: PaymentIntent,
    /// Where to send the bank transfer, once `/pay` has been called.
    pub deposit: Option<Deposit>,
    /// Network the payment settles on (`stellar-testnet` or `simulated`).
    pub ledger: &'static str,
    pub stellar_transactions: Vec<StellarTxView>,
    pub payout: Option<Payout>,
    pub refund: Option<Refund>,
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

/// A bank transfer reported by a collection provider.
#[derive(Debug, Deserialize)]
pub struct DepositEvent {
    pub event_id: String,
    /// The deposit `reference` from the payment's instructions (virtual account number).
    pub reference: String,
    pub amount_minor: i64,
    pub currency: crate::domain::Currency,
    /// The account the money came from, if the provider reports it. Refunds go back here.
    #[serde(default)]
    pub sender: Option<SenderAccount>,
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
        fee_source_amount: quote.fee_source_amount,
        send_amount: quote.send_amount,
        expected_payout: quote.destination_amount,
        exchange_rate: quote.exchange_rate,
        conversion_path: quote.conversion_path,
        recipient_rail: rail,
        recipient_masked: req.recipient.masked_reference(),
        recipient: Json(req.recipient),
        recipient_name: resolved.account_name,
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

pub async fn get_details(state: &AppState, payment: PaymentIntent) -> AppResult<PaymentDetails> {
    let id = payment.id.as_str();
    let (deposit, txs, payout, refund, events) = tokio::try_join!(
        db::get_deposit(&state.pool, id),
        db::list_stellar_txs(&state.pool, id),
        db::get_payout(&state.pool, id),
        db::get_refund(&state.pool, id),
        db::list_audit(&state.pool, id),
    )?;
    let stellar_transactions = txs
        .into_iter()
        .map(|tx| StellarTxView {
            explorer_url: state.ledger.explorer_url(&tx.tx_hash),
            tx,
        })
        .collect();
    Ok(PaymentDetails {
        payment,
        deposit,
        ledger: state.ledger.name(),
        stellar_transactions,
        payout,
        refund,
        events,
    })
}

/// Returns the bank transfer instructions for a payment, creating them on first call.
/// Safe to call repeatedly.
pub async fn pay(state: &AppState, payment: &PaymentIntent) -> AppResult<Deposit> {
    let id = payment.id.as_str();
    match payment.status {
        PaymentStatus::AwaitingPayment => return existing_deposit(state, id).await,
        PaymentStatus::Quoted => {}
        other => {
            return Err(AppError::Conflict(format!(
                "payment is {}; no deposit instructions can be issued",
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

    let collection = state
        .collections
        .get(payment.source_currency)
        .ok_or_else(|| AppError::Validation(format!("{} deposits are not collected", payment.source_currency)))?;
    let expires_at = Utc::now() + Duration::seconds(state.config.collections.deposit_expiry_secs);
    let instructions = collection
        .create_deposit(DepositRequest {
            idempotency_key: id,
            currency: payment.source_currency,
            amount_minor: payment.source_amount,
            expires_at,
        })
        .await?;
    let deposit = Deposit {
        payment_intent_id: payment.id.clone(),
        provider: collection.provider().to_string(),
        currency: payment.source_currency,
        expected_amount: payment.source_amount,
        reference: instructions.reference,
        bank_name: instructions.bank_name,
        account_number: instructions.account_number,
        account_name: instructions.account_name,
        provider_reference: instructions.provider_reference,
        status: DepositStatus::Awaiting,
        received_amount: None,
        sender_account: None,
        expires_at,
        received_at: None,
        created_at: Utc::now(),
    };

    let mut tx = state.pool.begin().await?;
    if let Err(err) = db::insert_deposit(&mut *tx, &deposit).await {
        if db::is_unique_violation(&err) {
            // A concurrent call won the race; return its instructions.
            drop(tx);
            return existing_deposit(state, id).await;
        }
        return Err(err.into());
    }
    let moved = db::transition(
        &mut tx,
        id,
        PaymentStatus::Quoted,
        PaymentStatus::AwaitingPayment,
        "deposit.instructions_issued",
        Some(&deposit.reference),
    )
    .await?;
    if !moved {
        return Err(AppError::Conflict("payment changed state; retry".into()));
    }
    db::set_payment_expiry(&mut *tx, id, expires_at).await?;
    tx.commit().await?;
    Ok(deposit)
}

async fn existing_deposit(state: &AppState, id: &str) -> AppResult<Deposit> {
    db::get_deposit(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow!("payment {id} is awaiting payment but has no deposit")))
}

/// Records a bank transfer and starts settlement (or the refund, if it can't be used).
pub async fn handle_deposit(state: &AppState, event: DepositEvent) -> AppResult<WebhookOutcome> {
    let mut tx = state.pool.begin().await?;
    if !db::record_webhook(&mut *tx, "collections", &event.event_id).await? {
        return Ok(WebhookOutcome::Duplicate);
    }
    let deposit = db::get_deposit_by_reference(&mut *tx, &event.reference)
        .await?
        .ok_or(AppError::NotFound("deposit"))?;
    if event.currency != deposit.currency {
        return Err(AppError::Validation(format!(
            "deposit is in {}, not {}",
            deposit.currency, event.currency
        )));
    }
    if event.amount_minor <= 0 {
        return Err(AppError::Validation("deposit amount must be positive".into()));
    }
    let id = deposit.payment_intent_id.clone();
    let detail = format!("{} {} via {}", event.amount_minor, event.currency, deposit.provider);
    if !db::mark_deposit_received(&mut *tx, &id, event.amount_minor, event.sender.as_ref(), Utc::now()).await? {
        // A second transfer to the same account. Real money: needs a person to return it.
        db::insert_audit(&mut *tx, &id, "deposit.duplicate", None, None, Some(&detail)).await?;
        tx.commit().await?;
        tracing::error!(payment_id = %id, %detail, "second deposit for one payment; needs manual return");
        return Ok(WebhookOutcome::Ignored);
    }
    let payment = db::get_payment(&mut *tx, &id)
        .await?
        .ok_or(AppError::NotFound("payment"))?;

    match payment.status {
        PaymentStatus::AwaitingPayment => {
            db::transition(
                &mut tx,
                &id,
                PaymentStatus::AwaitingPayment,
                PaymentStatus::PaymentDetected,
                "deposit.received",
                Some(&detail),
            )
            .await?;
            if event.amount_minor < deposit.expected_amount {
                let reason = format!(
                    "received {} but the payment needs {}",
                    event.amount_minor, deposit.expected_amount
                );
                fail_to_refund(
                    &mut tx,
                    &id,
                    PaymentStatus::PaymentDetected,
                    "deposit.underpaid",
                    &reason,
                )
                .await?;
            } else if event.amount_minor > deposit.expected_amount {
                let extra = format!("{} more than expected", event.amount_minor - deposit.expected_amount);
                db::insert_audit(&mut *tx, &id, "deposit.overpaid", None, None, Some(&extra)).await?;
            }
        }
        PaymentStatus::Expired => {
            db::transition(
                &mut tx,
                &id,
                PaymentStatus::Expired,
                PaymentStatus::RefundPending,
                "deposit.late",
                Some(&detail),
            )
            .await?;
            db::set_failure_reason(&mut *tx, &id, "the bank transfer arrived after the payment expired").await?;
        }
        other => {
            db::insert_audit(&mut *tx, &id, "deposit.unexpected", None, None, Some(other.as_str())).await?;
            tx.commit().await?;
            return Ok(WebhookOutcome::Ignored);
        }
    }
    tx.commit().await?;
    advance(state, &id).await?;
    Ok(WebhookOutcome::Processed)
}

/// `from` -> FAILED -> REFUND_PENDING, inside an open transaction.
async fn fail_to_refund(
    tx: &mut sqlx::SqliteConnection,
    id: &str,
    from: PaymentStatus,
    event: &str,
    reason: &str,
) -> AppResult<bool> {
    if !db::transition(tx, id, from, PaymentStatus::Failed, event, Some(reason)).await? {
        return Ok(false);
    }
    db::set_failure_reason(&mut *tx, id, reason).await?;
    db::transition(
        tx,
        id,
        PaymentStatus::Failed,
        PaymentStatus::RefundPending,
        "refund.pending",
        None,
    )
    .await?;
    Ok(true)
}

/// Takes the next step for a payment, as far as it can go right now. Idempotent.
pub async fn advance(state: &AppState, id: &str) -> AppResult<()> {
    // Each pass moves the payment at least one state forward, or stops.
    for _ in 0..4 {
        let Some(payment) = db::get_payment(&state.pool, id).await? else {
            return Ok(());
        };
        if !step(state, &payment).await? {
            return Ok(());
        }
    }
    Ok(())
}

/// One step; returns true if the payment's status moved and another step may follow.
async fn step(state: &AppState, payment: &PaymentIntent) -> AppResult<bool> {
    let id = payment.id.as_str();
    match payment.status {
        PaymentStatus::PaymentDetected => settle(state, payment).await,
        PaymentStatus::Completed => {
            redeem(state, payment).await?;
            Ok(false)
        }
        PaymentStatus::RefundPending | PaymentStatus::Refunded => {
            reverse(state, payment).await?;
            if payment.status == PaymentStatus::RefundPending && db::get_refund(&state.pool, id).await?.is_none() {
                match refunds::start_automatic(state, payment).await {
                    // Another caller (webhook or recovery) started it first.
                    Err(AppError::Conflict(_)) => {}
                    other => {
                        other?;
                    }
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

async fn settle(state: &AppState, payment: &PaymentIntent) -> AppResult<bool> {
    let id = payment.id.as_str();
    let (source, destination) = (payment.source_currency, payment.destination_currency);
    let stroops = |minor, c| minor_to_stroops(minor, c).ok_or_else(|| AppError::Internal(anyhow!("amount overflow")));
    let op = LedgerOp::Settle {
        source,
        destination,
        mint: stroops(payment.source_amount, source)?,
        send: stroops(payment.send_amount, source)?,
        dest_min: stroops(payment.expected_payout, destination)?,
    };
    match ledger::execute(state, id, op).await? {
        ledger::Step::Succeeded(tx) => finalize_settlement(state, payment, &tx).await,
        ledger::Step::Failed(reason) => {
            let reason = format!("Stellar settlement failed: {reason}");
            let mut conn = state.pool.begin().await?;
            let moved = fail_to_refund(
                &mut conn,
                id,
                PaymentStatus::PaymentDetected,
                "stellar.settle_failed",
                &reason,
            )
            .await?;
            conn.commit().await?;
            Ok(moved)
        }
        ledger::Step::Pending => Ok(false),
    }
}

/// PAYMENT_DETECTED -> SETTLED -> PAYOUT_PENDING, then exactly one payout request.
async fn finalize_settlement(state: &AppState, payment: &PaymentIntent, settle: &StellarTx) -> AppResult<bool> {
    let id = payment.id.as_str();
    let adapter = adapter_for(state, payment)?;
    let mut tx = state.pool.begin().await?;
    if !db::transition(
        &mut tx,
        id,
        PaymentStatus::PaymentDetected,
        PaymentStatus::Settled,
        "stellar.settled",
        Some(&settle.tx_hash),
    )
    .await?
    {
        return Ok(false);
    }
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
    submit_payout(state, payment, adapter.as_ref(), 0).await?;
    Ok(true)
}

/// The settlement's delivered destination tokens, if it succeeded.
async fn settled_tx(state: &AppState, id: &str) -> AppResult<Option<StellarTx>> {
    Ok(db::get_live_stellar_tx(&state.pool, id, StellarTxKind::Settle)
        .await?
        .filter(|t| t.status == StellarTxStatus::Succeeded))
}

/// After a completed payout: burn the destination tokens (the fiat has left our account).
async fn redeem(state: &AppState, payment: &PaymentIntent) -> AppResult<()> {
    let Some(settle) = settled_tx(state, &payment.id).await? else {
        return Ok(());
    };
    let Some(amount) = settle.delivered.filter(|d| *d > 0) else {
        return Ok(());
    };
    let op = LedgerOp::Redeem {
        currency: payment.destination_currency,
        amount,
    };
    if let ledger::Step::Succeeded(tx) = ledger::execute(state, &payment.id, op).await? {
        audit_once(state, &payment.id, "stellar.redeemed", &tx.tx_hash).await?;
    }
    Ok(())
}

/// After a failed or reversed payout: convert the destination tokens back and burn them, and
/// burn the fee, so the treasury holds nothing for this payment.
async fn reverse(state: &AppState, payment: &PaymentIntent) -> AppResult<()> {
    let Some(settle) = settled_tx(state, &payment.id).await? else {
        return Ok(());
    };
    // A payout reversed after completion: its tokens were already burned on redeem.
    if db::get_live_stellar_tx(&state.pool, &payment.id, StellarTxKind::Redeem)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let LedgerOp::Settle { mint, send, .. } = settle.operation.0 else {
        return Ok(());
    };
    let op = LedgerOp::Reverse {
        from: payment.destination_currency,
        to: payment.source_currency,
        amount: settle.delivered.unwrap_or(0),
        burn: mint - send,
    };
    if let ledger::Step::Succeeded(tx) = ledger::execute(state, &payment.id, op).await? {
        audit_once(state, &payment.id, "stellar.reversed", &tx.tx_hash).await?;
    }
    Ok(())
}

async fn audit_once(state: &AppState, id: &str, event: &str, detail: &str) -> AppResult<()> {
    let events = db::list_audit(&state.pool, id).await?;
    if !events.iter().any(|e| e.event_type == event) {
        db::insert_audit(&state.pool, id, event, None, None, Some(detail)).await?;
    }
    Ok(())
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
        // Rail confirmed delivery synchronously; no webhook will follow.
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

/// PAYOUT_PENDING -> FAILED -> REFUND_PENDING: the sender's money is owed back.
pub async fn fail_payout(state: &AppState, payment_id: &str, event: &str, reason: &str) -> AppResult<()> {
    tracing::error!(%payment_id, %reason, "payout failed");
    let mut tx = state.pool.begin().await?;
    if fail_to_refund(&mut tx, payment_id, PaymentStatus::PayoutPending, event, reason).await? {
        db::update_payout_status(&mut *tx, payment_id, PayoutStatus::Failed, Some(reason), Utc::now()).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Provider callbacks, for payouts and for refunds (which use the same rails).
pub async fn handle_payout_event(state: &AppState, event: PayoutEvent) -> AppResult<WebhookOutcome> {
    let Some(payout) = db::get_payout_by_reference(&state.pool, &event.provider_reference).await? else {
        return refunds::handle_event(state, event).await;
    };
    let mut tx = state.pool.begin().await?;
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
            let ok = fail_to_refund(&mut tx, id, PaymentStatus::PayoutPending, "payout.failed", reason).await?;
            if ok {
                db::update_payout_status(&mut *tx, id, PayoutStatus::Failed, Some(reason), now).await?;
            }
            ok
        }
        PayoutEventStatus::Reversed => {
            // NGN addendum A.6: a reversal after success re-opens the payment for refund.
            let reason = event.reason.as_deref().unwrap_or("payout reversed by provider");
            let ok = fail_to_refund(&mut tx, id, PaymentStatus::Completed, "payout.reversed", reason).await?;
            if ok {
                db::update_payout_status(&mut *tx, id, PayoutStatus::Reversed, Some(reason), now).await?;
            }
            ok
        }
    };
    tx.commit().await?;
    if applied {
        advance(state, id).await?;
    }
    Ok(if applied {
        WebhookOutcome::Processed
    } else {
        WebhookOutcome::Ignored
    })
}

/// AWAITING_PAYMENT past its deadline -> EXPIRED. Returns how many expired.
pub async fn expire_unpaid(state: &AppState) -> AppResult<usize> {
    let mut expired = 0;
    for id in db::list_expired_awaiting(&state.pool, Utc::now()).await? {
        let mut tx = state.pool.begin().await?;
        if db::transition(
            &mut tx,
            &id,
            PaymentStatus::AwaitingPayment,
            PaymentStatus::Expired,
            "deposit.expired",
            None,
        )
        .await?
        {
            db::mark_deposit_expired(&mut *tx, &id).await?;
            expired += 1;
        }
        tx.commit().await?;
    }
    Ok(expired)
}
