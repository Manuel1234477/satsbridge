//! Payouts to a Lightning Address (LUD-16 / LNURL-pay LUD-06) whose owner off-ramps to local
//! currency, e.g. Tapnob (`name@tapnob.io` -> Naira in the owner's linked bank account).
//!
//! SatsBridge sends the net sats; the provider converts at its own rate, so the quoted
//! destination amount is an estimate. Delivery is confirmed by the Lightning payment itself.
//! Only allowlisted domains are contacted.

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{Institution, InstitutionKind, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient};
use crate::{
    adapters::{
        AdapterError,
        lightning::{LightningAdapter, decode_invoice},
    },
    domain::{Currency, Rail, Recipient},
};

pub struct LightningAddressConfig {
    pub provider: &'static str,
    pub display_name: &'static str,
    pub currency: Currency,
    /// Lower-case domains (and their subdomains) we will call, e.g. `tapnob.io`.
    pub allowed_domains: Vec<String>,
    /// Only for tests against a local server.
    pub allow_http: bool,
}

pub struct LightningAddressAdapter {
    http: reqwest::Client,
    lightning: Arc<dyn LightningAdapter>,
    config: LightningAddressConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayParams {
    tag: Option<String>,
    callback: Option<String>,
    min_sendable: Option<u64>,
    max_sendable: Option<u64>,
    metadata: Option<String>,
    status: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallbackResponse {
    pr: Option<String>,
    status: Option<String>,
    reason: Option<String>,
}

struct ValidParams {
    callback: Url,
    min_msat: u64,
    max_msat: u64,
    metadata: String,
}

/// Routing fee budget: 0.5% with a 10 sat floor.
fn max_fee_sats(amount_sats: i64) -> i64 {
    (amount_sats / 200).max(10)
}

impl LightningAddressAdapter {
    pub fn new(http: reqwest::Client, lightning: Arc<dyn LightningAdapter>, config: LightningAddressConfig) -> Self {
        Self {
            http,
            lightning,
            config,
        }
    }

    pub fn tapnob(http: reqwest::Client, lightning: Arc<dyn LightningAdapter>, domains: Vec<String>) -> Self {
        Self::new(
            http,
            lightning,
            LightningAddressConfig {
                provider: "tapnob",
                display_name: "Tapnob",
                currency: Currency::Ngn,
                allowed_domains: domains,
                allow_http: false,
            },
        )
    }

    fn is_allowed_host(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.config.allowed_domains.iter().any(|d| {
            let d = d.split(':').next().unwrap_or(d);
            host == d || host.ends_with(&format!(".{d}"))
        })
    }

    /// Splits `name@domain`, enforcing LUD-16 username characters and the domain allowlist.
    fn parse_address<'a>(&self, address: &'a str) -> Result<(&'a str, &'a str), AdapterError> {
        let invalid = |m: &str| AdapterError::InvalidRecipient(m.to_string());
        let (user, domain) = address
            .trim()
            .split_once('@')
            .ok_or_else(|| invalid("expected name@domain"))?;
        let user_ok = !user.is_empty()
            && user
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.+".contains(&b));
        if !user_ok {
            return Err(invalid("invalid Lightning Address name"));
        }
        let host = domain.split(':').next().unwrap_or(domain);
        if !self.is_allowed_host(host) {
            return Err(invalid(&format!("Lightning Address domain {domain} is not supported")));
        }
        Ok((user, domain))
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: Url) -> Result<T, AdapterError> {
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AdapterError::Unavailable(format!("{} request failed: {e}", self.config.display_name)))?;
        let status = res.status();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| AdapterError::Unavailable(e.to_string()))?;
        // LNURL servers report errors as {"status":"ERROR"} with any HTTP status, so parse first.
        serde_json::from_slice(&bytes).map_err(|_| {
            AdapterError::Unavailable(format!(
                "{} returned HTTP {status} with an unexpected body",
                self.config.display_name
            ))
        })
    }

    async fn pay_params(&self, address: &str) -> Result<ValidParams, AdapterError> {
        let (user, domain) = self.parse_address(address)?;
        let scheme = if self.config.allow_http { "http" } else { "https" };
        let url = Url::parse(&format!("{scheme}://{domain}/.well-known/lnurlp/{user}"))
            .map_err(|_| AdapterError::InvalidRecipient("invalid Lightning Address".into()))?;
        let params: PayParams = self.get_json(url).await?;

        if params.status.as_deref() == Some("ERROR") {
            tracing::info!(address, reason = ?params.reason, "Lightning Address lookup failed");
            return Err(AdapterError::RecipientNotFound);
        }
        let bad = |m: &str| AdapterError::Unavailable(format!("invalid LNURL-pay response: {m}"));
        if params.tag.as_deref() != Some("payRequest") {
            return Err(bad("tag is not payRequest"));
        }
        let callback = Url::parse(params.callback.as_deref().ok_or_else(|| bad("missing callback"))?)
            .map_err(|_| bad("callback is not a URL"))?;
        let scheme_ok = callback.scheme() == "https" || (self.config.allow_http && callback.scheme() == "http");
        if !scheme_ok || !callback.host_str().is_some_and(|h| self.is_allowed_host(h)) {
            return Err(bad("callback is not on an allowed https host"));
        }
        let (min_msat, max_msat) = match (params.min_sendable, params.max_sendable) {
            (Some(min), Some(max)) if min >= 1 && min <= max => (min, max),
            _ => return Err(bad("invalid minSendable/maxSendable")),
        };
        let metadata = params.metadata.ok_or_else(|| bad("missing metadata"))?;
        Ok(ValidParams {
            callback,
            min_msat,
            max_msat,
            metadata,
        })
    }
}

/// LUD-06: the invoice must be for exactly the requested amount and commit to the metadata.
pub fn verify_invoice(pr: &str, amount_msat: u64, metadata: &str) -> Result<(), AdapterError> {
    let bad = |m: &str| AdapterError::Rejected(format!("invoice from Lightning Address rejected: {m}"));
    let invoice = decode_invoice(pr).map_err(|_| bad("not a valid BOLT11 invoice"))?;
    if invoice.amount_msat != Some(amount_msat) {
        return Err(bad("amount does not match the request"));
    }
    if invoice.description_hash.as_deref() != Some(hex::encode(Sha256::digest(metadata)).as_str()) {
        return Err(bad("description hash does not match metadata"));
    }
    if invoice.expires_at_unix <= chrono::Utc::now().timestamp().max(0) as u64 {
        return Err(bad("invoice already expired"));
    }
    Ok(())
}

fn address_of(recipient: &Recipient) -> Result<String, AdapterError> {
    match recipient {
        Recipient::LightningAddress { address } => Ok(address.trim().to_ascii_lowercase()),
        _ => Err(AdapterError::InvalidRecipient(
            "expected a LIGHTNING_ADDRESS recipient".into(),
        )),
    }
}

#[async_trait]
impl PayoutAdapter for LightningAddressAdapter {
    fn provider(&self) -> &'static str {
        self.config.provider
    }

    fn currency(&self) -> Currency {
        self.config.currency
    }

    fn rail(&self) -> Rail {
        Rail::LightningAddress
    }

    fn guarantees_destination_amount(&self) -> bool {
        false
    }

    /// A retry would fetch a fresh invoice and could pay twice, so unconfirmed payouts go to review.
    fn safe_to_retry(&self) -> bool {
        false
    }

    async fn institutions(&self, _country: Option<&str>) -> Result<Vec<Institution>, AdapterError> {
        Ok(self
            .config
            .allowed_domains
            .iter()
            .map(|d| Institution {
                code: d.clone(),
                name: self.config.display_name.into(),
                kind: InstitutionKind::Wallet,
            })
            .collect())
    }

    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        let params = self.pay_params(&address_of(recipient)?).await?;
        Ok(ResolvedRecipient {
            account_name: None,
            min_sats: Some(params.min_msat.div_ceil(1000) as i64),
            max_sats: Some((params.max_msat / 1000) as i64),
            ..Default::default()
        })
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        let params = self.pay_params(&address_of(request.recipient)?).await?;
        let amount_msat =
            u64::try_from(request.amount_sats).map_err(|_| AdapterError::Rejected("invalid amount".into()))? * 1000;
        if !(params.min_msat..=params.max_msat).contains(&amount_msat) {
            return Err(AdapterError::Rejected(format!(
                "amount {amount_msat} msat is outside the address limits {}..{}",
                params.min_msat, params.max_msat
            )));
        }

        let mut callback = params.callback;
        callback
            .query_pairs_mut()
            .append_pair("amount", &amount_msat.to_string());
        let response: CallbackResponse = self.get_json(callback).await?;
        if response.status.as_deref() == Some("ERROR") {
            return Err(AdapterError::Rejected(format!(
                "{} refused the payment: {}",
                self.config.display_name,
                response.reason.unwrap_or_default()
            )));
        }
        let pr = response
            .pr
            .ok_or_else(|| AdapterError::Unavailable("LNURL callback returned no invoice".into()))?;
        verify_invoice(&pr, amount_msat, &params.metadata)?;

        // TODO: a real node must refuse to pay twice for the same idempotency key.
        let sent = self
            .lightning
            .pay_invoice(&pr, max_fee_sats(request.amount_sats))
            .await?;
        tracing::info!(payment_id = request.idempotency_key, payment_hash = %sent.payment_hash, fee_sats = sent.fee_sats, "Lightning Address payout sent");
        Ok(PayoutAccepted {
            provider_reference: sent.payment_hash,
            completed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, extract::Query, routing::get};
    use bitcoin::{
        hashes::{Hash, sha256},
        secp256k1::{Secp256k1, SecretKey},
    };
    use lightning_invoice::{Currency as LnCurrency, InvoiceBuilder, PaymentSecret};
    use serde_json::{Value, json};

    use super::*;
    use crate::adapters::lightning::{Invoice, InvoiceDescription, InvoiceStatus, PaymentSent};

    /// Node that pays any valid invoice.
    struct PayingNode;

    #[async_trait]
    impl LightningAdapter for PayingNode {
        fn name(&self) -> &'static str {
            "paying-node"
        }

        async fn create_invoice(&self, _: i64, _: &InvoiceDescription, _: i64) -> Result<Invoice, AdapterError> {
            unreachable!("payouts never create invoices")
        }

        async fn pay_invoice(&self, pr: &str, _: i64) -> Result<PaymentSent, AdapterError> {
            let hash = decode_invoice(pr).unwrap().payment_hash;
            Ok(PaymentSent {
                payment_hash: hash,
                preimage: String::new(),
                fee_sats: 0,
            })
        }

        async fn invoice_status(&self, _: &str) -> Result<InvoiceStatus, AdapterError> {
            Ok(InvoiceStatus::Pending)
        }
    }

    const METADATA: &str = r#"[["text/plain","Pay ada@tapnob.io"],["text/identifier","ada@tapnob.io"]]"#;

    fn invoice(amount_msat: u64, metadata: &str) -> String {
        let key = SecretKey::from_slice(&[7; 32]).unwrap();
        InvoiceBuilder::new(LnCurrency::Regtest)
            .description_hash(sha256::Hash::hash(metadata.as_bytes()))
            .payment_hash(sha256::Hash::hash(b"preimage"))
            .payment_secret(PaymentSecret([1; 32]))
            .current_timestamp()
            .min_final_cltv_expiry_delta(144)
            .amount_milli_satoshis(amount_msat)
            .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
            .unwrap()
            .to_string()
    }

    /// Fake LNURL server. `?amount=` echoes an invoice for that amount; `lie=1` returns a wrong amount.
    async fn server() -> String {
        let app = Router::new()
            .route(
                "/.well-known/lnurlp/{user}",
                get(|axum::extract::Path(user): axum::extract::Path<String>, headers: axum::http::HeaderMap| async move {
                    let host = headers["host"].to_str().unwrap().to_string();
                    if user != "ada" {
                        return Json(json!({"status": "ERROR", "reason": "invalid username"}));
                    }
                    Json(json!({
                        "tag": "payRequest",
                        "callback": format!("http://{host}/lnurlp/ada/callback"),
                        "minSendable": 1_000, "maxSendable": 100_000_000_000u64,
                        "metadata": METADATA,
                    }))
                }),
            )
            .route(
                "/lnurlp/ada/callback",
                get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                    let amount: u64 = q["amount"].parse().unwrap();
                    Json::<Value>(json!({"pr": invoice(amount, METADATA), "routes": []}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr.to_string()
    }

    fn adapter(domain: &str) -> LightningAddressAdapter {
        LightningAddressAdapter::new(
            reqwest::Client::new(),
            Arc::new(PayingNode),
            LightningAddressConfig {
                provider: "tapnob",
                display_name: "Tapnob",
                currency: Currency::Ngn,
                allowed_domains: vec![domain.to_string()],
                allow_http: true,
            },
        )
    }

    fn to(address: String) -> Recipient {
        Recipient::LightningAddress { address }
    }

    #[tokio::test]
    async fn resolves_and_pays_an_address() {
        let domain = server().await;
        let ln = adapter(&domain);
        let resolved = ln.resolve_recipient(&to(format!("ada@{domain}"))).await.unwrap();
        assert_eq!((resolved.min_sats, resolved.max_sats), (Some(1), Some(100_000_000)));

        let r = to(format!("ada@{domain}"));
        let accepted = ln
            .create_payout(PayoutRequest {
                idempotency_key: "pay_1",
                attempt: 1,
                recipient: &r,
                account_name: None,
                amount_minor: 22_000_000,
                amount_sats: 164_999,
                currency: Currency::Ngn,
                narration: "x".into(),
            })
            .await
            .unwrap();
        assert!(accepted.completed);
        assert_eq!(accepted.provider_reference, sha256::Hash::hash(b"preimage").to_string());
    }

    #[tokio::test]
    async fn rejects_unknown_users_and_foreign_domains() {
        let domain = server().await;
        let ln = adapter(&domain);
        assert!(matches!(
            ln.resolve_recipient(&to(format!("bob@{domain}"))).await,
            Err(AdapterError::RecipientNotFound)
        ));
        assert!(matches!(
            ln.resolve_recipient(&to("ada@evil.example".into())).await,
            Err(AdapterError::InvalidRecipient(_))
        ));
        assert!(matches!(
            ln.resolve_recipient(&to("Not An Address".into())).await,
            Err(AdapterError::InvalidRecipient(_))
        ));
    }

    #[test]
    fn verifies_invoice_amount_and_metadata() {
        assert!(verify_invoice(&invoice(5_000, METADATA), 5_000, METADATA).is_ok());
        assert!(verify_invoice(&invoice(4_000, METADATA), 5_000, METADATA).is_err());
        assert!(verify_invoice(&invoice(5_000, "[]"), 5_000, METADATA).is_err());
        assert!(verify_invoice("lnbc1garbage", 5_000, METADATA).is_err());
    }

    #[test]
    fn allowlist_accepts_subdomains_only() {
        let ln = adapter("tapnob.io");
        assert!(ln.is_allowed_host("tapnob.io"));
        assert!(ln.is_allowed_host("api.tapnob.io"));
        assert!(!ln.is_allowed_host("tapnob.io.evil.com"));
        assert!(!ln.is_allowed_host("eviltapnob.io"));
    }
}
