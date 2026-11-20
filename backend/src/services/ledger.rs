//! Runs Stellar transactions for payments, exactly once.
//!
//! Each step (`settle`, `redeem`, `reverse`) is signed, **stored**, then submitted, so a crash or
//! timeout never loses track of a transaction that might have been applied. An unknown outcome is
//! resolved later by looking the hash up; resubmitting the same envelope is harmless (a sequence
//! number can only be used once). A transaction counts as never applied only when the network
//! says so, or when it is still missing after its `maxTime` has passed; only then is a new one
//! built. At most one live transaction per payment and kind is allowed (a partial unique index).

use chrono::{Duration, Utc};
use sqlx::types::Json;

use crate::{
    adapters::{
        AdapterError,
        stellar::{LedgerOp, PreparedTx, TxOutcome},
    },
    db::{self, StellarTx, StellarTxKind, StellarTxStatus},
    error::{AppError, AppResult},
    state::AppState,
};

/// After `maxTime`, wait this long for ledger close and Horizon ingestion before calling a
/// missing transaction "never applied".
const NOT_FOUND_GRACE_SECS: i64 = 30;

#[derive(Debug)]
pub enum Step {
    Succeeded(StellarTx),
    /// Definitely not applied, and won't be retried automatically.
    Failed(String),
    /// Outcome unknown, or the network can't take it right now; recovery will follow up.
    Pending,
}

fn kind_of(op: &LedgerOp) -> StellarTxKind {
    match op {
        LedgerOp::Settle { .. } => StellarTxKind::Settle,
        LedgerOp::Redeem { .. } => StellarTxKind::Redeem,
        LedgerOp::Reverse { .. } => StellarTxKind::Reverse,
    }
}

/// Memo on every transaction: the payment ID, as far as it fits in 28 bytes.
fn memo(payment_id: &str) -> String {
    payment_id.chars().take(28).collect()
}

/// Runs `op` for a payment, or reports on the live transaction of that kind if there is one.
pub async fn execute(state: &AppState, payment_id: &str, op: LedgerOp) -> AppResult<Step> {
    let kind = kind_of(&op);
    // One treasury, one sequence number: build and submit one transaction at a time.
    let _guard = state.ledger_lock.lock().await;

    if let Some(live) = db::get_live_stellar_tx(&state.pool, payment_id, kind).await? {
        return resolve_locked(state, live).await;
    }

    let prepared = match state.ledger.prepare(&op, &memo(payment_id)).await {
        Ok(p) => p,
        Err(AdapterError::Unavailable(reason)) => {
            tracing::warn!(payment_id, kind = ?kind, %reason, "cannot build Stellar transaction yet");
            db::insert_audit(&state.pool, payment_id, "stellar.deferred", None, None, Some(&reason)).await?;
            return Ok(Step::Pending);
        }
        Err(err) => return Ok(Step::Failed(err.to_string())),
    };

    let row = StellarTx {
        id: 0,
        payment_intent_id: payment_id.to_string(),
        kind,
        tx_hash: prepared.hash.clone(),
        envelope_xdr: prepared.envelope_xdr.clone(),
        operation: Json(op),
        status: StellarTxStatus::Submitted,
        delivered: None,
        ledger: None,
        error: None,
        valid_until: prepared.valid_until,
        created_at: Utc::now(),
        confirmed_at: None,
    };
    if let Err(err) = db::insert_stellar_tx(&state.pool, &row).await {
        if db::is_unique_violation(&err)
            && let Some(live) = db::get_live_stellar_tx(&state.pool, payment_id, kind).await?
        {
            return resolve_locked(state, live).await;
        }
        return Err(err.into());
    }
    db::insert_audit(
        &state.pool,
        payment_id,
        &format!("stellar.{}_submitted", row.operation.kind()),
        None,
        None,
        Some(&prepared.hash),
    )
    .await?;

    match state.ledger.submit(&prepared).await {
        Ok(outcome) => record(state, &row, outcome).await,
        Err(err) => {
            tracing::warn!(payment_id, hash = %prepared.hash, error = %err, "Stellar submission unconfirmed");
            Ok(Step::Pending)
        }
    }
}

/// Finds out what happened to a submitted transaction (for the recovery worker).
pub async fn resolve(state: &AppState, tx: StellarTx) -> AppResult<Step> {
    let _guard = state.ledger_lock.lock().await;
    let current = db::get_stellar_tx_by_hash(&state.pool, &tx.tx_hash)
        .await?
        .unwrap_or(tx);
    resolve_locked(state, current).await
}

async fn resolve_locked(state: &AppState, tx: StellarTx) -> AppResult<Step> {
    match tx.status {
        StellarTxStatus::Succeeded => return Ok(Step::Succeeded(tx)),
        StellarTxStatus::Failed => return Ok(Step::Failed(tx.error.unwrap_or_default())),
        StellarTxStatus::Submitted => {}
    }
    match state.ledger.lookup(&tx.tx_hash).await {
        Ok(TxOutcome::NotFound) => {}
        Ok(outcome) => return record(state, &tx, outcome).await,
        Err(err) => {
            tracing::warn!(hash = %tx.tx_hash, error = %err, "Stellar lookup failed");
            return Ok(Step::Pending);
        }
    }
    if Utc::now() > tx.valid_until + Duration::seconds(NOT_FOUND_GRACE_SECS) {
        // Past maxTime and not on the ledger: it can never apply. Free the slot for a rebuild.
        let reason = "not on the ledger before it expired";
        db::finish_stellar_tx(
            &state.pool,
            &tx.tx_hash,
            StellarTxStatus::Failed,
            None,
            None,
            Some(reason),
        )
        .await?;
        db::insert_audit(
            &state.pool,
            &tx.payment_intent_id,
            "stellar.tx_expired",
            None,
            None,
            Some(&tx.tx_hash),
        )
        .await?;
        // Expiry is not a business failure: the caller may build a new one.
        return Ok(Step::Pending);
    }
    // Still within its window: resubmitting the same envelope is safe.
    let prepared = PreparedTx {
        hash: tx.tx_hash.clone(),
        envelope_xdr: tx.envelope_xdr.clone(),
        valid_until: tx.valid_until,
    };
    match state.ledger.submit(&prepared).await {
        Ok(outcome) => record(state, &tx, outcome).await,
        Err(_) => Ok(Step::Pending),
    }
}

async fn record(state: &AppState, tx: &StellarTx, outcome: TxOutcome) -> AppResult<Step> {
    let id = tx.payment_intent_id.as_str();
    let kind = tx.operation.kind();
    match outcome {
        TxOutcome::Succeeded { ledger, delivered } => {
            db::finish_stellar_tx(
                &state.pool,
                &tx.tx_hash,
                StellarTxStatus::Succeeded,
                delivered,
                Some(ledger),
                None,
            )
            .await?;
            tracing::info!(payment_id = id, kind, hash = %tx.tx_hash, ledger, "Stellar transaction applied");
            db::get_stellar_tx_by_hash(&state.pool, &tx.tx_hash)
                .await?
                .map(Step::Succeeded)
                .ok_or_else(|| AppError::Internal(anyhow::anyhow!("stellar tx {} vanished", tx.tx_hash)))
        }
        TxOutcome::Failed { reason } => {
            db::finish_stellar_tx(
                &state.pool,
                &tx.tx_hash,
                StellarTxStatus::Failed,
                None,
                None,
                Some(&reason),
            )
            .await?;
            tracing::warn!(payment_id = id, kind, hash = %tx.tx_hash, %reason, "Stellar transaction failed");
            db::insert_audit(
                &state.pool,
                id,
                &format!("stellar.{kind}_failed"),
                None,
                None,
                Some(&reason),
            )
            .await?;
            // Operations that failed on the ledger (`tx_failed`, e.g. the DEX moved below the
            // quote) end a settlement. Transaction-level rejections (bad sequence, too late, fee)
            // and any redeem/reverse failure are rebuilt by recovery.
            Ok(if kind == "settle" && reason.starts_with("tx_failed") {
                Step::Failed(reason)
            } else {
                Step::Pending
            })
        }
        TxOutcome::NotFound => Ok(Step::Pending),
    }
}
