use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::AdapterError;

pub mod lnbits;

#[derive(Debug, PartialEq, Eq)]
pub enum InvoiceStatus {
    Pending,
    Paid {
        amount_sats: i64,
    },
    /// Failed, expired, or unknown to the node.
    Failed,
}

pub struct Invoice {
    pub payment_request: String,
    pub payment_hash: String,
    pub expires_at: DateTime<Utc>,
}

pub struct PaymentSent {
    pub payment_hash: String,
    pub preimage: String,
    pub fee_sats: i64,
}

/// Wraps a Lightning node (LND, CLN, LNbits, ...). Incoming settlement arrives via
/// `POST /api/v1/webhooks/lightning`.
#[async_trait]
pub trait LightningAdapter: Send + Sync {
    fn name(&self) -> &'static str;

    async fn create_invoice(&self, amount_sats: i64, memo: &str, expiry_secs: i64) -> Result<Invoice, AdapterError>;

    /// Pays a BOLT11 invoice. Return `Rejected` only when the payment definitely failed; an
    /// in-flight or unknown outcome must be `Unavailable` so it is never treated as safe to refund.
    async fn pay_invoice(&self, payment_request: &str, max_fee_sats: i64) -> Result<PaymentSent, AdapterError>;

    /// Status of an invoice this node issued, looked up by payment hash.
    async fn invoice_status(&self, payment_hash: &str) -> Result<InvoiceStatus, AdapterError>;
}

/// Produces fake BOLT11-looking strings and pretends outgoing payments succeed.
/// Use the dev routes to simulate incoming settlement.
pub struct MockLightningAdapter;

#[async_trait]
impl LightningAdapter for MockLightningAdapter {
    fn name(&self) -> &'static str {
        "mock-lightning"
    }

    async fn create_invoice(&self, amount_sats: i64, _memo: &str, expiry_secs: i64) -> Result<Invoice, AdapterError> {
        let preimage = Uuid::new_v4();
        let payment_hash = hex::encode(Sha256::digest(preimage.as_bytes()));
        // 1 sat = 10 nBTC in BOLT11 amount notation.
        let payment_request = format!("lnbcrt{}n1mock{}", amount_sats * 10, &payment_hash[..32]);
        Ok(Invoice {
            payment_request,
            payment_hash,
            expires_at: Utc::now() + Duration::seconds(expiry_secs),
        })
    }

    async fn pay_invoice(&self, payment_request: &str, _max_fee_sats: i64) -> Result<PaymentSent, AdapterError> {
        let payment_hash = match payment_request.parse::<lightning_invoice::Bolt11Invoice>() {
            Ok(invoice) => invoice.payment_hash().to_string(),
            Err(_) => hex::encode(Sha256::digest(payment_request.as_bytes())),
        };
        Ok(PaymentSent {
            payment_hash,
            preimage: "00".repeat(32),
            fee_sats: 0,
        })
    }

    /// Never paid on its own; use the dev settle route.
    async fn invoice_status(&self, _payment_hash: &str) -> Result<InvoiceStatus, AdapterError> {
        Ok(InvoiceStatus::Pending)
    }
}
