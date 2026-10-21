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
        "INSERT INTO quotes (id, source_currency, destination_currency, destination_rail,
            payout_amount_guaranteed, amount_side, source_amount,
            destination_amount, amount_sats, fee_sats, fee_source_amount, fee_bps, exchange_rate,
            rate_source, rate_as_of, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&q.id)
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
        "INSERT INTO payment_intents (id, quote_id, source_currency, destination_currency,
            source_amount, expected_payout, amount_sats, fee_sats, exchange_rate, recipient,
            recipient_rail, recipient_masked, recipient_name, status, failure_reason, created_at,
            updated_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&p.id)
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
            failure_reason, created_at, completed_at, reversed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
