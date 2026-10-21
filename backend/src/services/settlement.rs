//! Settlement watcher: asks the Lightning node about invoices still awaiting payment, so
//! settlement doesn't depend on the node being able to reach our webhook. Also expires
//! invoices that ran out unpaid.

use std::time::Duration;

use chrono::Utc;

use super::payments::{self, LightningEvent, WebhookOutcome};
use crate::{
    adapters::lightning::InvoiceStatus,
    db,
    domain::PaymentStatus,
    error::{AppError, AppResult},
    state::AppState,
};

/// Event id shared by the poller and node webhooks, so the two paths deduplicate each other.
pub fn status_event_id(payment_hash: &str) -> String {
    format!("status:{payment_hash}")
}

/// Confirms an invoice with the node and, if it is paid, runs the normal settlement path.
pub async fn confirm_settlement(state: &AppState, payment_hash: &str) -> AppResult<WebhookOutcome> {
    match state.lightning.invoice_status(payment_hash).await? {
        InvoiceStatus::Paid { amount_sats } => {
            let event = LightningEvent {
                event_id: status_event_id(payment_hash),
                payment_hash: payment_hash.into(),
                amount_sats,
            };
            payments::handle_lightning_settled(state, event).await
        }
        InvoiceStatus::Pending | InvoiceStatus::Failed => Ok(WebhookOutcome::Ignored),
    }
}

/// One pass over invoices awaiting payment. Returns how many were settled.
pub async fn poll_once(state: &AppState) -> AppResult<usize> {
    let mut settled = 0;
    for lightning in db::list_awaiting_lightning(&state.pool, 200).await? {
        let hash = lightning.payment_hash.as_str();
        match state.lightning.invoice_status(hash).await {
            Ok(InvoiceStatus::Paid { .. }) => match confirm_settlement(state, hash).await {
                Ok(WebhookOutcome::Processed) => settled += 1,
                Ok(_) => {}
                Err(err) => tracing::error!(payment_hash = hash, error = %err, "settling paid invoice failed"),
            },
            Ok(_) if lightning.expires_at <= Utc::now() => {
                let mut conn = state.pool.acquire().await?;
                db::transition(
                    &mut conn,
                    &lightning.payment_intent_id,
                    PaymentStatus::AwaitingPayment,
                    PaymentStatus::Expired,
                    "invoice.expired",
                    Some(hash),
                )
                .await
                .map_err(AppError::Internal)?;
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(payment_hash = hash, error = %err, "invoice status check failed"),
        }
    }
    Ok(settled)
}

pub fn spawn_watcher(state: AppState, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(err) = poll_once(&state).await {
                tracing::error!(error = %err, "settlement watcher pass failed");
            }
        }
    })
}
