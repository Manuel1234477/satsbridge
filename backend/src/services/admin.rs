//! Operations views and actions for `/api/v1/admin/*` (PRD §15 "basic analytics and
//! operational dashboard", §19 success metrics).

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::recovery::{self, Outcome};
use crate::{
    db::{self, CorridorStats, PaymentIntent, Payout, StatusCount},
    domain::{Currency, PaymentStatus, Rail},
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct RailInfo {
    pub currency: Currency,
    pub rail: Rail,
    pub provider: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub generated_at: DateTime<Utc>,
    pub total_payments: i64,
    /// COMPLETED / (COMPLETED + FAILED + REFUND_PENDING + REFUNDED). Abandoned (expired)
    /// payments and ones still in progress are left out.
    pub completion_rate: Option<f64>,
    pub statuses: Vec<StatusCount>,
    pub corridors: Vec<CorridorStats>,
    pub avg_settlement_secs: Option<f64>,
    pub avg_payout_secs: Option<f64>,
    pub needs_review: i64,
    pub refund_pending: i64,
    pub lightning: &'static str,
    pub rails: Vec<RailInfo>,
}

#[derive(Debug, Serialize)]
pub struct AdminPayment {
    #[serde(flatten)]
    pub payment: PaymentIntent,
    pub payout: Option<Payout>,
}

pub async fn summary(state: &AppState) -> AppResult<Summary> {
    let (statuses, corridors, timings) = tokio::try_join!(
        db::status_counts(&state.pool),
        db::corridor_stats(&state.pool),
        db::timings(&state.pool)
    )?;
    let count = |s: PaymentStatus| statuses.iter().find(|c| c.status == s).map_or(0, |c| c.count);
    let completed = count(PaymentStatus::Completed);
    let finished =
        completed + count(PaymentStatus::Failed) + count(PaymentStatus::RefundPending) + count(PaymentStatus::Refunded);
    let mut rails: Vec<RailInfo> = Currency::ALL
        .into_iter()
        .flat_map(|c| state.payouts.for_currency(c))
        .map(|a| RailInfo {
            currency: a.currency(),
            rail: a.rail(),
            provider: a.provider(),
        })
        .collect();
    rails.sort_by_key(|r| (r.currency, r.rail.as_str()));
    Ok(Summary {
        generated_at: Utc::now(),
        total_payments: statuses.iter().map(|c| c.count).sum(),
        completion_rate: (finished > 0).then(|| completed as f64 / finished as f64),
        refund_pending: count(PaymentStatus::RefundPending),
        statuses,
        corridors,
        avg_settlement_secs: timings.avg_settlement_secs,
        avg_payout_secs: timings.avg_payout_secs,
        needs_review: timings.needs_review,
        lightning: state.lightning.name(),
        rails,
    })
}

pub async fn list(
    state: &AppState,
    status: Option<PaymentStatus>,
    needs_review: bool,
    limit: i64,
) -> AppResult<Vec<AdminPayment>> {
    let mut out = Vec::new();
    for payment in db::list_payments_filtered(&state.pool, status, needs_review, limit).await? {
        let payout = db::get_payout(&state.pool, &payment.id).await?;
        out.push(AdminPayment { payment, payout });
    }
    Ok(out)
}

pub async fn get(state: &AppState, id: &str) -> AppResult<PaymentIntent> {
    db::get_payment(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("payment"))
}

/// Clears the review flag and retries now. `force` is for when someone has checked the
/// provider by hand and confirmed the earlier attempt didn't pay.
pub async fn retry_payout(state: &AppState, id: &str, force: bool) -> AppResult<&'static str> {
    if !db::release_payout_for_retry(&state.pool, id).await? {
        return Err(AppError::Conflict("no unsubmitted payout for this payment".into()));
    }
    db::insert_audit(
        &state.pool,
        id,
        "admin.retry_payout",
        None,
        None,
        force.then_some("forced"),
    )
    .await?;
    let payout = db::get_payout(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("payout"))?;
    Ok(match recovery::recover_payout(state, &payout, force).await? {
        Outcome::Resubmitted => "resubmitted",
        Outcome::Reconciled => "found an earlier accepted attempt",
        Outcome::Flagged => "flagged for review",
        Outcome::Skipped => "skipped",
    })
}

/// Gives up on the payout and moves the payment to REFUND_PENDING, after someone confirmed
/// with the provider that nothing was paid.
pub async fn fail_payout(state: &AppState, id: &str, reason: &str) -> AppResult<()> {
    let payout = db::get_payout(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("payout"))?;
    if payout.status != db::PayoutStatus::Pending {
        return Err(AppError::Conflict(
            "only payouts the provider never accepted can be failed by hand".into(),
        ));
    }
    let reason = format!("marked failed by admin: {reason}");
    let mut tx = state.pool.begin().await?;
    if db::transition(
        &mut tx,
        id,
        PaymentStatus::PayoutPending,
        PaymentStatus::Failed,
        "admin.fail_payout",
        Some(&reason),
    )
    .await?
    {
        db::update_payout_status(&mut *tx, id, db::PayoutStatus::Failed, Some(&reason), Utc::now()).await?;
        db::set_failure_reason(&mut *tx, id, &reason).await?;
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
    tx.commit().await?;
    Ok(())
}
