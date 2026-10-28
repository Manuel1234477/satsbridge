//! Recovery worker: finishes payouts that the normal path left unfinished.
//!
//! 1. **Unsubmitted payouts** (crash before submission, or an unconfirmed attempt). Before any
//!    resubmission it asks the provider whether an earlier attempt already went through
//!    (`find_payout`). Rails that can't answer that (`safe_to_retry() == false`) go to manual
//!    review instead. Retries back off exponentially and stop after `PAYOUT_MAX_ATTEMPTS`.
//! 2. **Unconfirmed payouts**: accepted, but no webhook after a while; asks the provider.
//! 3. **Refunds in flight** whose Lightning outcome was unknown.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{
    payments::{self, PayoutEvent},
    refunds,
};
use crate::{
    config::RecoveryConfig,
    db::{self, Payout},
    error::AppResult,
    state::AppState,
};

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct RecoveryReport {
    pub resubmitted: usize,
    pub reconciled: usize,
    pub confirmed: usize,
    pub flagged: usize,
    pub refunds_completed: usize,
}

fn backoff(cfg: &RecoveryConfig, attempts: i64) -> chrono::Duration {
    let exp = attempts.saturating_sub(1).clamp(0, 10) as u32;
    chrono::Duration::seconds(cfg.base_backoff_secs.saturating_mul(2i64.pow(exp)).min(6 * 3600))
}

fn is_due(cfg: &RecoveryConfig, payout: &Payout, now: DateTime<Utc>) -> bool {
    payout
        .last_attempt_at
        .is_none_or(|at| now - at >= backoff(cfg, payout.attempts))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Resubmitted,
    Reconciled,
    Flagged,
    Skipped,
}

/// Tries to move one unsubmitted payout forward. `force` (admin) skips the safe-to-retry check,
/// for when someone has confirmed by hand that the earlier attempt didn't pay.
pub async fn recover_payout(state: &AppState, payout: &Payout, force: bool) -> AppResult<Outcome> {
    let cfg = &state.config.recovery;
    let id = payout.payment_intent_id.as_str();
    let Some(payment) = db::get_payment(&state.pool, id).await? else {
        return Ok(Outcome::Skipped);
    };
    let flag = |reason: String| async move {
        tracing::warn!(payment_id = id, %reason, "payout needs review");
        db::flag_payout_review(&state.pool, id, &reason)
            .await
            .map(|_| Outcome::Flagged)
    };
    let adapter = match payments::adapter_for(state, &payment) {
        Ok(adapter) => adapter,
        Err(err) => return Ok(flag(err.to_string()).await?),
    };
    if payout.attempts >= cfg.max_attempts && !force {
        return Ok(flag(format!("gave up after {} attempts", payout.attempts)).await?);
    }
    // attempts == 0: nothing was ever sent (the claim precedes every call), so it is always safe.
    if payout.attempts > 0 {
        if !adapter.safe_to_retry() && !force {
            return Ok(flag(format!(
                "{} payouts can't be retried automatically; check the provider",
                adapter.provider()
            ))
            .await?);
        }
        match adapter.find_payout(id).await {
            Ok(Some(accepted)) => {
                payments::record_accepted(state, id, &accepted).await?;
                return Ok(Outcome::Reconciled);
            }
            Ok(None) => {}
            Err(err) => {
                // Can't tell whether an earlier attempt paid: don't risk paying twice.
                tracing::warn!(payment_id = id, error = %err, "payout reconciliation failed; will retry later");
                return Ok(Outcome::Skipped);
            }
        }
    }
    Ok(
        if payments::submit_payout(state, &payment, adapter.as_ref(), payout.attempts).await? {
            Outcome::Resubmitted
        } else {
            Outcome::Skipped
        },
    )
}

pub async fn run_once(state: &AppState) -> AppResult<RecoveryReport> {
    let cfg = state.config.recovery;
    let now = Utc::now();
    let mut report = RecoveryReport::default();

    for payout in db::list_unsubmitted_payouts(&state.pool).await? {
        if !is_due(&cfg, &payout, now) {
            continue;
        }
        match recover_payout(state, &payout, false).await {
            Ok(Outcome::Resubmitted) => report.resubmitted += 1,
            Ok(Outcome::Reconciled) => report.reconciled += 1,
            Ok(Outcome::Flagged) => report.flagged += 1,
            Ok(Outcome::Skipped) => {}
            Err(err) => tracing::error!(payment_id = %payout.payment_intent_id, error = %err, "payout recovery failed"),
        }
    }

    let before = now - chrono::Duration::seconds(cfg.unconfirmed_after_secs);
    for payout in db::list_unconfirmed_payouts(&state.pool, before).await? {
        let (Some(reference), Some(payment)) = (
            payout.provider_reference.as_deref(),
            db::get_payment(&state.pool, &payout.payment_intent_id).await?,
        ) else {
            continue;
        };
        let Ok(adapter) = payments::adapter_for(state, &payment) else {
            continue;
        };
        match adapter.payout_status(reference).await {
            Ok(Some(status)) => {
                let event = PayoutEvent {
                    event_id: format!("status:{reference}:{status:?}"),
                    provider_reference: reference.to_string(),
                    status,
                    reason: Some("status checked with provider".into()),
                };
                payments::handle_payout_event(state, event).await?;
                report.confirmed += 1;
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(%reference, error = %err, "payout status check failed"),
        }
    }

    report.refunds_completed = refunds::check_in_flight(state).await?;
    Ok(report)
}

pub fn spawn_worker(state: AppState, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match run_once(&state).await {
                Ok(report) if report != RecoveryReport::default() => tracing::info!(?report, "recovery pass"),
                Ok(_) => {}
                Err(err) => tracing::error!(error = %err, "recovery pass failed"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let cfg = RecoveryConfig::defaults();
        assert_eq!(backoff(&cfg, 0).num_seconds(), 120);
        assert_eq!(backoff(&cfg, 1).num_seconds(), 120);
        assert_eq!(backoff(&cfg, 3).num_seconds(), 480);
        assert_eq!(backoff(&cfg, 30).num_seconds(), 6 * 3600);
    }
}
