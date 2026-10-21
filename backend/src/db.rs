//! Persistence models and queries. Every status change goes through [`transition`], which is a
//! compare-and-set on the current status so concurrent or repeated events cannot apply twice.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection, SqliteExecutor, types::Json};

use crate::domain::{Currency, PaymentStatus, Rail, Recipient};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum AmountSide {
    /// `amount` is what the sender pays, in the source currency.
    #[default]
    Source,
    /// `amount` is what the recipient receives, in the destination currency.
    Destination,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Quote {
    pub id: String,
    #[serde(skip_serializing)]
    pub user_id: Option<String>,
    pub source_currency: Currency,
    pub destination_currency: Currency,
    pub destination_rail: Rail,
    /// False when the payout provider converts at its own rate (destination_amount is an estimate).
    pub payout_amount_guaranteed: bool,
    pub amount_side: AmountSide,
    pub source_amount: i64,
    pub destination_amount: i64,
    pub amount_sats: i64,
    pub fee_sats: i64,
    pub fee_source_amount: i64,
    pub fee_bps: i64,
    /// Destination units per 1 source unit (mid rate, before fees).
    pub exchange_rate: String,
    pub rate_source: String,
    /// When the oldest of the two BTC prices used was fetched.
    pub rate_as_of: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct PaymentIntent {
    pub id: String,
    #[serde(skip_serializing)]
    pub user_id: Option<String>,
    pub quote_id: String,
    pub source_currency: Currency,
    pub destination_currency: Currency,
    pub source_amount: i64,
    pub expected_payout: i64,
    pub amount_sats: i64,
    pub fee_sats: i64,
    pub exchange_rate: String,
    /// Full account details; needed to execute the payout, never returned by the API.
    #[serde(skip_serializing)]
    pub recipient: Json<Recipient>,
    pub recipient_rail: Rail,
    pub recipient_masked: String,
    pub recipient_name: Option<String>,
    pub status: PaymentStatus,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Hash of the Lightning refund to the sender; set while a refund is in flight or done.
    pub refund_payment_hash: Option<String>,
    pub refund_sats: Option<i64>,
    pub refunded_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum LightningStatus {
    Pending,
    Settled,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct LightningPayment {
    pub payment_intent_id: String,
    pub payment_request: String,
    pub payment_hash: String,
    pub amount_sats: i64,
    pub status: LightningStatus,
    pub expires_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum PayoutStatus {
    /// Row reserved; not yet accepted by the provider.
    Pending,
    Submitted,
    Completed,
    Failed,
    Reversed,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Payout {
    pub payment_intent_id: String,
    pub provider: String,
    pub rail: Rail,
    pub destination_currency: Currency,
    pub destination_amount: i64,
    pub destination_reference: String,
    pub account_name: Option<String>,
    pub provider_reference: Option<String>,
    pub status: PayoutStatus,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub reversed_at: Option<DateTime<Utc>>,
    /// Submission attempts so far; each one is claimed atomically (see `claim_payout_attempt`).
    pub attempts: i64,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub needs_review: bool,
    pub review_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct AuditEvent {
    pub id: i64,
    pub payment_intent_id: String,
    pub event_type: String,
    pub from_status: Option<PaymentStatus>,
    pub to_status: Option<PaymentStatus>,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct RecipientResolution {
    pub id: String,
    pub currency: Currency,
    pub bank_code: String,
    pub account_number_hash: String,
    pub account_name: String,
    pub provider: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error().is_some_and(|e| e.is_unique_violation())
}

// --- quotes ---

pub async fn insert_quote<'e>(db: impl SqliteExecutor<'e>, q: &Quote) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO quotes (id, user_id, source_currency, destination_currency, destination_rail,
            payout_amount_guaranteed, amount_side, source_amount,
            destination_amount, amount_sats, fee_sats, fee_source_amount, fee_bps, exchange_rate,
            rate_source, rate_as_of, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&q.id)
    .bind(&q.user_id)
    .bind(q.source_currency)
    .bind(q.destination_currency)
    .bind(q.destination_rail)
    .bind(q.payout_amount_guaranteed)
    .bind(q.amount_side)
    .bind(q.source_amount)
    .bind(q.destination_amount)
    .bind(q.amount_sats)
    .bind(q.fee_sats)
    .bind(q.fee_source_amount)
    .bind(q.fee_bps)
    .bind(&q.exchange_rate)
    .bind(&q.rate_source)
    .bind(q.rate_as_of)
    .bind(q.expires_at)
    .bind(q.created_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_quote<'e>(db: impl SqliteExecutor<'e>, id: &str) -> sqlx::Result<Option<Quote>> {
    sqlx::query_as("SELECT * FROM quotes WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}

// --- payment intents ---

pub async fn insert_payment<'e>(db: impl SqliteExecutor<'e>, p: &PaymentIntent) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO payment_intents (id, user_id, quote_id, source_currency, destination_currency,
            source_amount, expected_payout, amount_sats, fee_sats, exchange_rate, recipient,
            recipient_rail, recipient_masked, recipient_name, status, failure_reason, created_at,
            updated_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&p.id)
    .bind(&p.user_id)
    .bind(&p.quote_id)
    .bind(p.source_currency)
    .bind(p.destination_currency)
    .bind(p.source_amount)
    .bind(p.expected_payout)
    .bind(p.amount_sats)
    .bind(p.fee_sats)
    .bind(&p.exchange_rate)
    .bind(&p.recipient)
    .bind(p.recipient_rail)
    .bind(&p.recipient_masked)
    .bind(&p.recipient_name)
    .bind(p.status)
    .bind(&p.failure_reason)
    .bind(p.created_at)
    .bind(p.updated_at)
    .bind(p.expires_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_payment<'e>(db: impl SqliteExecutor<'e>, id: &str) -> sqlx::Result<Option<PaymentIntent>> {
    sqlx::query_as("SELECT * FROM payment_intents WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn list_payments<'e>(db: impl SqliteExecutor<'e>, limit: i64) -> sqlx::Result<Vec<PaymentIntent>> {
    sqlx::query_as("SELECT * FROM payment_intents ORDER BY created_at DESC LIMIT ?")
        .bind(limit)
        .fetch_all(db)
        .await
}

pub async fn set_payment_expiry<'e>(
    db: impl SqliteExecutor<'e>,
    id: &str,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE payment_intents SET expires_at = ? WHERE id = ?")
        .bind(expires_at)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_failure_reason<'e>(db: impl SqliteExecutor<'e>, id: &str, reason: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE payment_intents SET failure_reason = ? WHERE id = ?")
        .bind(reason)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Moves a payment from `from` to `to` only if it is still in `from`, and records an audit event.
/// Returns `false` (and writes nothing) if another event already moved it.
pub async fn transition(
    conn: &mut SqliteConnection,
    id: &str,
    from: PaymentStatus,
    to: PaymentStatus,
    event_type: &str,
    detail: Option<&str>,
) -> anyhow::Result<bool> {
    anyhow::ensure!(from.can_transition_to(to), "illegal transition {from:?} -> {to:?}");
    let now = Utc::now();
    let updated = sqlx::query("UPDATE payment_intents SET status = ?, updated_at = ? WHERE id = ? AND status = ?")
        .bind(to)
        .bind(now)
        .bind(id)
        .bind(from)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if updated == 0 {
        return Ok(false);
    }
    insert_audit(&mut *conn, id, event_type, Some(from), Some(to), detail).await?;
    tracing::info!(
        payment_id = id,
        from = from.as_str(),
        to = to.as_str(),
        event_type,
        "payment transition"
    );
    Ok(true)
}

// --- lightning ---

pub async fn insert_lightning<'e>(db: impl SqliteExecutor<'e>, l: &LightningPayment) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO lightning_payments (payment_intent_id, payment_request, payment_hash,
            amount_sats, status, expires_at, settled_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&l.payment_intent_id)
    .bind(&l.payment_request)
    .bind(&l.payment_hash)
    .bind(l.amount_sats)
    .bind(l.status)
    .bind(l.expires_at)
    .bind(l.settled_at)
    .bind(l.created_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_lightning_by_payment<'e>(
    db: impl SqliteExecutor<'e>,
    payment_id: &str,
) -> sqlx::Result<Option<LightningPayment>> {
    sqlx::query_as("SELECT * FROM lightning_payments WHERE payment_intent_id = ?")
        .bind(payment_id)
        .fetch_optional(db)
        .await
}

pub async fn get_lightning_by_hash<'e>(
    db: impl SqliteExecutor<'e>,
    payment_hash: &str,
) -> sqlx::Result<Option<LightningPayment>> {
    sqlx::query_as("SELECT * FROM lightning_payments WHERE payment_hash = ?")
        .bind(payment_hash)
        .fetch_optional(db)
        .await
}

pub async fn mark_lightning_settled<'e>(
    db: impl SqliteExecutor<'e>,
    payment_id: &str,
    settled_at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE lightning_payments SET status = ?, settled_at = ? WHERE payment_intent_id = ?")
        .bind(LightningStatus::Settled)
        .bind(settled_at)
        .bind(payment_id)
        .execute(db)
        .await?;
    Ok(())
}

// --- payouts ---

pub async fn insert_payout<'e>(db: impl SqliteExecutor<'e>, p: &Payout) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO payouts (payment_intent_id, provider, rail, destination_currency,
            destination_amount, destination_reference, account_name, provider_reference, status,
            failure_reason, created_at, completed_at, reversed_at, attempts, last_attempt_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&p.payment_intent_id)
    .bind(&p.provider)
    .bind(p.rail)
    .bind(p.destination_currency)
    .bind(p.destination_amount)
    .bind(&p.destination_reference)
    .bind(&p.account_name)
    .bind(&p.provider_reference)
    .bind(p.status)
    .bind(&p.failure_reason)
    .bind(p.created_at)
    .bind(p.completed_at)
    .bind(p.reversed_at)
    .bind(p.attempts)
    .bind(p.last_attempt_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_payout<'e>(db: impl SqliteExecutor<'e>, payment_id: &str) -> sqlx::Result<Option<Payout>> {
    sqlx::query_as("SELECT * FROM payouts WHERE payment_intent_id = ?")
        .bind(payment_id)
        .fetch_optional(db)
        .await
}

pub async fn get_payout_by_reference<'e>(
    db: impl SqliteExecutor<'e>,
    provider_reference: &str,
) -> sqlx::Result<Option<Payout>> {
    sqlx::query_as("SELECT * FROM payouts WHERE provider_reference = ?")
        .bind(provider_reference)
        .fetch_optional(db)
        .await
}

pub async fn mark_payout_submitted<'e>(
    db: impl SqliteExecutor<'e>,
    payment_id: &str,
    provider_reference: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE payouts SET status = ?, provider_reference = ? WHERE payment_intent_id = ?")
        .bind(PayoutStatus::Submitted)
        .bind(provider_reference)
        .bind(payment_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn update_payout_status<'e>(
    db: impl SqliteExecutor<'e>,
    payment_id: &str,
    status: PayoutStatus,
    failure_reason: Option<&str>,
    at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE payouts SET status = ?,
            failure_reason = COALESCE(?, failure_reason),
            completed_at = CASE WHEN ? = 'completed' THEN ? ELSE completed_at END,
            reversed_at = CASE WHEN ? = 'reversed' THEN ? ELSE reversed_at END
         WHERE payment_intent_id = ?",
    )
    .bind(status)
    .bind(failure_reason)
    .bind(status)
    .bind(at)
    .bind(status)
    .bind(at)
    .bind(payment_id)
    .execute(db)
    .await?;
    Ok(())
}

// --- resolutions ---

pub async fn insert_resolution<'e>(db: impl SqliteExecutor<'e>, r: &RecipientResolution) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO recipient_resolutions (id, currency, bank_code, account_number_hash,
            account_name, provider, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&r.id)
    .bind(r.currency)
    .bind(&r.bank_code)
    .bind(&r.account_number_hash)
    .bind(&r.account_name)
    .bind(&r.provider)
    .bind(r.created_at)
    .bind(r.expires_at)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_resolution<'e>(db: impl SqliteExecutor<'e>, id: &str) -> sqlx::Result<Option<RecipientResolution>> {
    sqlx::query_as("SELECT * FROM recipient_resolutions WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}

// --- audit & webhooks ---

pub async fn insert_audit<'e>(
    db: impl SqliteExecutor<'e>,
    payment_id: &str,
    event_type: &str,
    from: Option<PaymentStatus>,
    to: Option<PaymentStatus>,
    detail: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO audit_events (payment_intent_id, event_type, from_status, to_status, detail, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(payment_id)
    .bind(event_type)
    .bind(from)
    .bind(to)
    .bind(detail)
    .bind(Utc::now())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_audit<'e>(db: impl SqliteExecutor<'e>, payment_id: &str) -> sqlx::Result<Vec<AuditEvent>> {
    sqlx::query_as("SELECT * FROM audit_events WHERE payment_intent_id = ? ORDER BY id")
        .bind(payment_id)
        .fetch_all(db)
        .await
}

/// Returns `false` if this event was already processed.
pub async fn record_webhook<'e>(db: impl SqliteExecutor<'e>, source: &str, event_id: &str) -> sqlx::Result<bool> {
    let inserted = sqlx::query(
        "INSERT INTO processed_webhooks (source, event_id, received_at) VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(source)
    .bind(event_id)
    .bind(Utc::now())
    .execute(db)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

/// Invoices whose payment is still awaiting settlement, oldest first.
pub async fn list_awaiting_lightning<'e>(
    db: impl SqliteExecutor<'e>,
    limit: i64,
) -> sqlx::Result<Vec<LightningPayment>> {
    sqlx::query_as(
        "SELECT l.* FROM lightning_payments l
         JOIN payment_intents p ON p.id = l.payment_intent_id
         WHERE p.status = ? AND l.status = ?
         ORDER BY l.created_at LIMIT ?",
    )
    .bind(PaymentStatus::AwaitingPayment)
    .bind(LightningStatus::Pending)
    .bind(limit)
    .fetch_all(db)
    .await
}

// --- users ---

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct User {
    pub id: String,
    pub contact: Option<String>,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_user<'e>(db: impl SqliteExecutor<'e>, u: &User) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO users (id, contact, token_hash, created_at) VALUES (?, ?, ?, ?)")
        .bind(&u.id)
        .bind(&u.contact)
        .bind(&u.token_hash)
        .bind(u.created_at)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_user_by_token_hash<'e>(db: impl SqliteExecutor<'e>, token_hash: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE token_hash = ?")
        .bind(token_hash)
        .fetch_optional(db)
        .await
}

pub async fn list_payments_for_user<'e>(
    db: impl SqliteExecutor<'e>,
    user_id: &str,
    limit: i64,
) -> sqlx::Result<Vec<PaymentIntent>> {
    sqlx::query_as("SELECT * FROM payment_intents WHERE user_id = ? ORDER BY created_at DESC LIMIT ?")
        .bind(user_id)
        .bind(limit)
        .fetch_all(db)
        .await
}

// --- payout recovery ---

/// Claims the next submission attempt. Only the caller that moves `attempts` from `expected`
/// may submit, so the settlement path and the recovery worker never submit concurrently.
pub async fn claim_payout_attempt(
    conn: &mut SqliteConnection,
    payment_id: &str,
    expected_attempts: i64,
) -> sqlx::Result<bool> {
    let claimed = sqlx::query(
        "UPDATE payouts SET attempts = attempts + 1, last_attempt_at = ?
         WHERE payment_intent_id = ? AND attempts = ? AND status = ? AND needs_review = 0",
    )
    .bind(Utc::now())
    .bind(payment_id)
    .bind(expected_attempts)
    .bind(PayoutStatus::Pending)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(claimed == 1)
}

/// Payouts the provider has not accepted yet, for payments still awaiting payout.
pub async fn list_unsubmitted_payouts<'e>(db: impl SqliteExecutor<'e>) -> sqlx::Result<Vec<Payout>> {
    sqlx::query_as(
        "SELECT o.* FROM payouts o JOIN payment_intents p ON p.id = o.payment_intent_id
         WHERE o.status = ? AND o.needs_review = 0 AND p.status = ?
         ORDER BY o.created_at LIMIT 100",
    )
    .bind(PayoutStatus::Pending)
    .bind(PaymentStatus::PayoutPending)
    .fetch_all(db)
    .await
}

/// Accepted payouts with no final status since `before`, to ask the provider about.
pub async fn list_unconfirmed_payouts<'e>(
    db: impl SqliteExecutor<'e>,
    before: DateTime<Utc>,
) -> sqlx::Result<Vec<Payout>> {
    sqlx::query_as(
        "SELECT o.* FROM payouts o JOIN payment_intents p ON p.id = o.payment_intent_id
         WHERE o.status = ? AND p.status = ? AND o.last_attempt_at < ?
         ORDER BY o.created_at LIMIT 100",
    )
    .bind(PayoutStatus::Submitted)
    .bind(PaymentStatus::PayoutPending)
    .bind(before)
    .fetch_all(db)
    .await
}

pub async fn flag_payout_review<'e>(db: impl SqliteExecutor<'e>, payment_id: &str, reason: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE payouts SET needs_review = 1, review_reason = ? WHERE payment_intent_id = ?")
        .bind(reason)
        .bind(payment_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Admin retry: clear the review flag and let the recovery worker try again immediately.
pub async fn release_payout_for_retry<'e>(db: impl SqliteExecutor<'e>, payment_id: &str) -> sqlx::Result<bool> {
    let updated = sqlx::query(
        "UPDATE payouts SET needs_review = 0, review_reason = NULL, last_attempt_at = NULL
         WHERE payment_intent_id = ? AND status = ?",
    )
    .bind(payment_id)
    .bind(PayoutStatus::Pending)
    .execute(db)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

// --- refunds ---

/// Takes the refund lock. Fails if another refund is already in flight or done.
pub async fn claim_refund<'e>(
    db: impl SqliteExecutor<'e>,
    payment_id: &str,
    payment_hash: &str,
    sats: i64,
) -> sqlx::Result<bool> {
    let claimed = sqlx::query(
        "UPDATE payment_intents SET refund_payment_hash = ?, refund_sats = ?
         WHERE id = ? AND status = ? AND refund_payment_hash IS NULL",
    )
    .bind(payment_hash)
    .bind(sats)
    .bind(payment_id)
    .bind(PaymentStatus::RefundPending)
    .execute(db)
    .await?
    .rows_affected();
    Ok(claimed == 1)
}

pub async fn release_refund<'e>(db: impl SqliteExecutor<'e>, payment_id: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE payment_intents SET refund_payment_hash = NULL, refund_sats = NULL WHERE id = ?")
        .bind(payment_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn mark_refunded<'e>(db: impl SqliteExecutor<'e>, payment_id: &str, at: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query("UPDATE payment_intents SET refunded_at = ? WHERE id = ?")
        .bind(at)
        .bind(payment_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn list_refunds_in_flight<'e>(db: impl SqliteExecutor<'e>) -> sqlx::Result<Vec<PaymentIntent>> {
    sqlx::query_as("SELECT * FROM payment_intents WHERE status = ? AND refund_payment_hash IS NOT NULL LIMIT 100")
        .bind(PaymentStatus::RefundPending)
        .fetch_all(db)
        .await
}

// --- admin ---

pub async fn list_payments_filtered<'e>(
    db: impl SqliteExecutor<'e>,
    status: Option<PaymentStatus>,
    needs_review: bool,
    limit: i64,
) -> sqlx::Result<Vec<PaymentIntent>> {
    sqlx::query_as(
        "SELECT p.* FROM payment_intents p LEFT JOIN payouts o ON o.payment_intent_id = p.id
         WHERE (?1 IS NULL OR p.status = ?1) AND (?2 = 0 OR o.needs_review = 1)
         ORDER BY p.created_at DESC LIMIT ?3",
    )
    .bind(status)
    .bind(needs_review)
    .bind(limit)
    .fetch_all(db)
    .await
}

#[derive(Debug, Serialize, FromRow)]
pub struct StatusCount {
    pub status: PaymentStatus,
    pub count: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CorridorStats {
    pub source_currency: Currency,
    pub destination_currency: Currency,
    pub payments: i64,
    pub completed: i64,
    /// Sum over completed payments, in destination minor units.
    pub delivered_minor: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Timings {
    /// Invoice created -> settled, over settled invoices.
    pub avg_settlement_secs: Option<f64>,
    /// Payout requested -> completed, over completed payouts.
    pub avg_payout_secs: Option<f64>,
    pub needs_review: i64,
}

pub async fn status_counts<'e>(db: impl SqliteExecutor<'e>) -> sqlx::Result<Vec<StatusCount>> {
    sqlx::query_as("SELECT status, COUNT(*) AS count FROM payment_intents GROUP BY status ORDER BY count DESC")
        .fetch_all(db)
        .await
}

pub async fn corridor_stats<'e>(db: impl SqliteExecutor<'e>) -> sqlx::Result<Vec<CorridorStats>> {
    sqlx::query_as(
        "SELECT source_currency, destination_currency, COUNT(*) AS payments,
                SUM(status = 'COMPLETED') AS completed,
                COALESCE(SUM(CASE WHEN status = 'COMPLETED' THEN expected_payout END), 0) AS delivered_minor
         FROM payment_intents GROUP BY source_currency, destination_currency ORDER BY payments DESC",
    )
    .fetch_all(db)
    .await
}

pub async fn timings<'e>(db: impl SqliteExecutor<'e>) -> sqlx::Result<Timings> {
    sqlx::query_as(
        "SELECT
            (SELECT AVG((julianday(settled_at) - julianday(created_at)) * 86400)
               FROM lightning_payments WHERE settled_at IS NOT NULL) AS avg_settlement_secs,
            (SELECT AVG((julianday(completed_at) - julianday(created_at)) * 86400)
               FROM payouts WHERE completed_at IS NOT NULL) AS avg_payout_secs,
            (SELECT COUNT(*) FROM payouts WHERE needs_review = 1) AS needs_review",
    )
    .fetch_one(db)
    .await
}
