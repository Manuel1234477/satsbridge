use async_trait::async_trait;
use chrono::{DateTime, Utc};

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

/// What an invoice describes itself as.
pub struct InvoiceDescription {
    /// Plain description, used when there is no metadata.
    pub memo: String,
    /// LNURL-pay metadata JSON. When set, the invoice commits to its SHA-256 (description hash),
    /// as LUD-06 requires, so the same invoice serves both BOLT11 and LNURL-pay.
    pub lnurl_metadata: Option<String>,
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

/// A Lightning node. Incoming payments only count once the node itself reports them paid
/// (see `services::settlement`); nothing settles on a caller's word.
#[async_trait]
pub trait LightningAdapter: Send + Sync {
    fn name(&self) -> &'static str;

    async fn create_invoice(
        &self,
        amount_sats: i64,
        description: &InvoiceDescription,
        expiry_secs: i64,
    ) -> Result<Invoice, AdapterError>;

    /// Pays a BOLT11 invoice. Return `Rejected` only when the payment definitely failed; an
    /// in-flight or unknown outcome must be `Unavailable` so it is never treated as safe to refund.
    async fn pay_invoice(&self, payment_request: &str, max_fee_sats: i64) -> Result<PaymentSent, AdapterError>;

    /// Status of an invoice this node issued, looked up by payment hash.
    async fn invoice_status(&self, payment_hash: &str) -> Result<InvoiceStatus, AdapterError>;
}

/// The BOLT11 fields we rely on.
#[derive(Debug)]
pub struct DecodedInvoice {
    pub payment_hash: String,
    pub amount_msat: Option<u64>,
    pub description_hash: Option<String>,
    pub expires_at_unix: u64,
}

/// Decodes a BOLT11 invoice and verifies its signature.
///
/// Deliberately skips `lightning-invoice`'s stricter semantic checks (e.g. a required feature
/// field), which reject invoices that LNbits and other nodes produce and pay without trouble.
pub fn decode_invoice(payment_request: &str) -> Result<DecodedInvoice, AdapterError> {
    let bad = |m: &str| AdapterError::Rejected(format!("invalid BOLT11 invoice: {m}"));
    let signed: lightning_invoice::SignedRawBolt11Invoice =
        payment_request.trim().parse().map_err(|_| bad("cannot be decoded"))?;
    if !signed.check_signature() {
        return Err(bad("signature does not verify"));
    }
    let raw = signed.raw_invoice();
    let payment_hash = raw
        .payment_hash()
        .ok_or_else(|| bad("missing payment hash"))?
        .0
        .to_string();
    // BOLT11 default expiry is one hour.
    let expiry_secs = raw.expiry_time().map(|e| e.as_seconds()).unwrap_or(3600);
    Ok(DecodedInvoice {
        payment_hash,
        // 1 msat = 10 pico-BTC.
        amount_msat: raw.amount_pico_btc().map(|pico| pico / 10),
        description_hash: raw.description_hash().map(|h| h.0.to_string()),
        expires_at_unix: raw.data.timestamp.as_unix_timestamp().saturating_add(expiry_secs),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issued by LNbits (FakeWallet backend): 21 sats, no BOLT11 feature field, which the
    /// `lightning-invoice` crate's strict parser rejects.
    const LNBITS_INVOICE: &str = "lnbc210n1p4dseqmdqzwsxqrrsssp57292udlmexkgd9t7rj0qytukuzshcdt68ugmec7m033sjned7cmqpp5xswelfc7k38k2g8z9nknhv9mkf3mvytmee4ascnx8kpe3ugf3xpqu2hmdvy5ja5z7zz3xje8vz5rmggzxd6km7mzv8xm2rj3ta0pckrsrjeufkrkexyhf6mxqpl2lcf8urh63hc3pnxjyru8shmhcux6edsqd7qewx";

    #[test]
    fn decodes_invoices_without_feature_bits() {
        let invoice = decode_invoice(LNBITS_INVOICE).unwrap();
        assert_eq!(invoice.amount_msat, Some(21_000));
        assert_eq!(invoice.payment_hash.len(), 64);
        assert!(LNBITS_INVOICE.parse::<lightning_invoice::Bolt11Invoice>().is_err());
    }

    #[test]
    fn rejects_garbage_and_tampered_invoices() {
        assert!(decode_invoice("lnbc1garbage").is_err());
        // Change one character of the data part: the checksum or the signature must fail.
        let mut tampered: Vec<char> = LNBITS_INVOICE.chars().collect();
        let i = tampered.len() / 2;
        tampered[i] = if tampered[i] == 'q' { 'p' } else { 'q' };
        assert!(decode_invoice(&tampered.into_iter().collect::<String>()).is_err());
    }
}
