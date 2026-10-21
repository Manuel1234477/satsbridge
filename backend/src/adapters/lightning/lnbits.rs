//! LNbits wallet (https://lnbits.com), API v1 (checked against LNbits 1.6).
//!
//! - Receive: `POST /api/v1/payments {out: false}` with the invoice key.
//! - Send: `POST /api/v1/payments {out: true, bolt11}` with the admin key.
//! - Status: `GET /api/v1/payments/{payment_hash}` -> `{paid, status, preimage, details}`.
//!
//! LNbits invoice webhooks are unsigned, so they are only a hint: settlement is always
//! confirmed by asking LNbits for the payment status.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Invoice, InvoiceStatus, LightningAdapter, PaymentSent};
use crate::adapters::AdapterError;

#[derive(Clone)]
pub struct LnbitsConfig {
    pub base_url: String,
    /// Can create invoices and read payments.
    pub invoice_key: String,
    /// Can spend. Needed only for Lightning Address payouts.
    pub admin_key: Option<String>,
    /// Public URL of `/api/v1/webhooks/lnbits`, attached to each invoice. Optional: the
    /// settlement watcher polls regardless.
    pub webhook_url: Option<String>,
}

pub struct LnbitsAdapter {
    http: reqwest::Client,
    config: LnbitsConfig,
    /// How long `pay_invoice` waits for a pending payment to resolve.
    pay_poll_attempts: u32,
    pay_poll_interval: Duration,
}

#[derive(Debug, Deserialize)]
struct CreatedPayment {
    payment_hash: String,
    #[serde(default)]
    payment_request: Option<String>,
    #[serde(default)]
    bolt11: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    preimage: Option<String>,
    /// Millisats; negative for outgoing.
    #[serde(default)]
    fee: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    paid: bool,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    preimage: Option<String>,
    #[serde(default)]
    details: Option<Details>,
}

#[derive(Debug, Deserialize)]
struct Details {
    /// Millisats; negative for outgoing.
    amount: i64,
    #[serde(default)]
    fee: i64,
}

enum Outcome {
    Paid { preimage: Option<String>, fee_msat: i64 },
    Pending,
    Failed,
}

fn unavailable(e: impl std::fmt::Display) -> AdapterError {
    AdapterError::Unavailable(format!("LNbits: {e}"))
}

/// FastAPI errors are `{"detail": "..."}` (or a list for validation errors).
fn detail(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("detail").cloned())
        .map(|d| d.as_str().map(str::to_string).unwrap_or_else(|| d.to_string()))
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(200).collect())
}

impl LnbitsAdapter {
    pub fn new(http: reqwest::Client, config: LnbitsConfig) -> Self {
        Self {
            http,
            config,
            pay_poll_attempts: 15,
            pay_poll_interval: Duration::from_secs(2),
        }
    }

    pub fn with_pay_polling(mut self, attempts: u32, interval: Duration) -> Self {
        self.pay_poll_attempts = attempts;
        self.pay_poll_interval = interval;
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.config.base_url.trim_end_matches('/'))
    }

    async fn post_payment(&self, key: &str, body: Value) -> Result<(StatusCode, Vec<u8>), AdapterError> {
        let res = self
            .http
            .post(self.url("/api/v1/payments"))
            .header("X-API-KEY", key)
            .json(&body)
            .send()
            .await
            .map_err(unavailable)?;
        let status = res.status();
        Ok((status, res.bytes().await.map_err(unavailable)?.to_vec()))
    }

    async fn outcome(&self, payment_hash: &str) -> Result<Outcome, AdapterError> {
        let res = self
            .http
            .get(self.url(&format!("/api/v1/payments/{payment_hash}")))
            .header("X-API-KEY", &self.config.invoice_key)
            .send()
            .await
            .map_err(unavailable)?;
        match res.status() {
            StatusCode::NOT_FOUND => return Ok(Outcome::Failed),
            s if !s.is_success() => {
                let body = res.bytes().await.unwrap_or_default();
                return Err(unavailable(format!("status lookup HTTP {s}: {}", detail(&body))));
            }
            _ => {}
        }
        let status: StatusResponse = res.json().await.map_err(unavailable)?;
        Ok(if status.paid {
            Outcome::Paid {
                preimage: status.preimage,
                fee_msat: status.details.map(|d| d.fee).unwrap_or_default(),
            }
        } else if status.status.as_deref() == Some("failed") {
            Outcome::Failed
        } else {
            Outcome::Pending
        })
    }
}

fn sent(payment_hash: String, preimage: Option<String>, fee_msat: i64) -> PaymentSent {
    PaymentSent {
        payment_hash,
        preimage: preimage.unwrap_or_default(),
        fee_sats: (fee_msat.unsigned_abs().div_ceil(1000)) as i64,
    }
}

#[async_trait]
impl LightningAdapter for LnbitsAdapter {
    fn name(&self) -> &'static str {
        "lnbits"
    }

    async fn create_invoice(&self, amount_sats: i64, memo: &str, expiry_secs: i64) -> Result<Invoice, AdapterError> {
        let mut body =
            json!({ "out": false, "amount": amount_sats, "unit": "sat", "memo": memo, "expiry": expiry_secs });
        if let Some(webhook) = &self.config.webhook_url {
            body["webhook"] = json!(webhook);
        }
        let (status, bytes) = self.post_payment(&self.config.invoice_key, body).await?;
        if !status.is_success() {
            let message = format!("LNbits invoice creation failed (HTTP {status}): {}", detail(&bytes));
            return Err(if status.is_server_error() {
                AdapterError::Unavailable(message)
            } else {
                AdapterError::Rejected(message)
            });
        }
        let created: CreatedPayment = serde_json::from_slice(&bytes).map_err(unavailable)?;
        let payment_request = created
            .payment_request
            .or(created.bolt11)
            .ok_or_else(|| unavailable("invoice response has no payment_request"))?;
        Ok(Invoice {
            payment_request,
            payment_hash: created.payment_hash,
            expires_at: Utc::now() + ChronoDuration::seconds(expiry_secs),
        })
    }

    async fn pay_invoice(&self, payment_request: &str, max_fee_sats: i64) -> Result<PaymentSent, AdapterError> {
        let admin_key = self
            .config
            .admin_key
            .as_deref()
            .ok_or_else(|| AdapterError::Rejected("LNBITS_ADMIN_KEY is required to send payments".into()))?;
        let payment_hash = payment_request
            .trim()
            .parse::<lightning_invoice::Bolt11Invoice>()
            .map_err(|_| AdapterError::Rejected("not a valid BOLT11 invoice".into()))?
            .payment_hash()
            .to_string();

        // LNbits has no per-payment fee cap; it reserves fees per its own settings.
        let mut failure = None;
        match self
            .post_payment(admin_key, json!({ "out": true, "bolt11": payment_request.trim() }))
            .await
        {
            Ok((status, bytes)) if status.is_success() => {
                if let Ok(created) = serde_json::from_slice::<CreatedPayment>(&bytes)
                    && created.status.as_deref() == Some("success")
                {
                    let paid = sent(payment_hash, created.preimage, created.fee.unwrap_or_default());
                    if paid.fee_sats > max_fee_sats {
                        tracing::warn!(fee_sats = paid.fee_sats, max_fee_sats, "Lightning fee above budget");
                    }
                    return Ok(paid);
                }
            }
            Ok((status, bytes)) => failure = Some(format!("HTTP {status}: {}", detail(&bytes))),
            Err(err) => failure = Some(err.to_string()),
        }

        // The POST may fail or time out while the payment is still in flight, so the payment
        // status is the source of truth.
        for attempt in 0..self.pay_poll_attempts.max(1) {
            match self.outcome(&payment_hash).await {
                Ok(Outcome::Paid { preimage, fee_msat }) => return Ok(sent(payment_hash, preimage, fee_msat)),
                Ok(Outcome::Failed) => {
                    return Err(AdapterError::Rejected(format!(
                        "Lightning payment failed: {}",
                        failure.unwrap_or_else(|| "no route or rejected".into())
                    )));
                }
                Ok(Outcome::Pending) | Err(_) if attempt + 1 < self.pay_poll_attempts => {
                    tokio::time::sleep(self.pay_poll_interval).await;
                }
                Ok(Outcome::Pending) | Err(_) => {}
            }
        }
        Err(AdapterError::Unavailable(format!(
            "Lightning payment {payment_hash} is still in flight"
        )))
    }

    async fn invoice_status(&self, payment_hash: &str) -> Result<InvoiceStatus, AdapterError> {
        let res = self
            .http
            .get(self.url(&format!("/api/v1/payments/{payment_hash}")))
            .header("X-API-KEY", &self.config.invoice_key)
            .send()
            .await
            .map_err(unavailable)?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(InvoiceStatus::Failed);
        }
        if !res.status().is_success() {
            return Err(unavailable(format!("status lookup HTTP {}", res.status())));
        }
        let status: StatusResponse = res.json().await.map_err(unavailable)?;
        Ok(match (status.paid, status.details) {
            (true, Some(d)) => InvoiceStatus::Paid {
                amount_sats: d.amount.unsigned_abs() as i64 / 1000,
            },
            (true, None) => {
                return Err(unavailable(
                    "paid invoice has no details; is the invoice key for this wallet?",
                ));
            }
            (false, _) if status.status.as_deref() == Some("failed") => InvoiceStatus::Failed,
            (false, _) => InvoiceStatus::Pending,
        })
    }
}

/// Extracts the payment hash from an LNbits webhook body. LNbits posts the payment as a
/// JSON-encoded string, so both `{...}` and `"{...}"` are accepted.
pub fn webhook_payment_hash(body: &[u8]) -> Option<String> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    if let Value::String(inner) = &value {
        value = serde_json::from_str(inner).ok()?;
    }
    let hash = value.get("payment_hash")?.as_str()?.to_ascii_lowercase();
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hash)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use bitcoin::{
        hashes::{Hash, sha256},
        secp256k1::{Secp256k1, SecretKey},
    };
    use lightning_invoice::{Currency as LnCurrency, InvoiceBuilder, PaymentSecret};
    use serde_json::{Value, json};

    use super::*;

    const INVOICE_KEY: &str = "inv-key";
    const ADMIN_KEY: &str = "admin-key";

    fn invoice(preimage: &[u8]) -> (String, String) {
        let key = SecretKey::from_slice(&[3; 32]).unwrap();
        let hash = sha256::Hash::hash(preimage);
        let pr = InvoiceBuilder::new(LnCurrency::Regtest)
            .description("t".into())
            .payment_hash(hash)
            .payment_secret(PaymentSecret([4; 32]))
            .current_timestamp()
            .min_final_cltv_expiry_delta(144)
            .amount_milli_satoshis(10_000)
            .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
            .unwrap()
            .to_string();
        (pr, hash.to_string())
    }

    /// Fake LNbits. Outgoing behaviour is chosen by the invoice's preimage seed:
    /// "ok" pays at once, "slow" is pending for 2 status checks, "fail" returns 520 and is unknown.
    #[derive(Clone, Default)]
    struct Fake {
        status_checks: Arc<AtomicUsize>,
    }

    async fn payments(headers: HeaderMap, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        let key = headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if body["out"] == false {
            if key != INVOICE_KEY {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"detail": "Invoice (or Admin) key required."})),
                );
            }
            let (pr, hash) = invoice(b"incoming");
            return (
                StatusCode::CREATED,
                Json(json!({"payment_hash": hash, "bolt11": pr, "status": "pending"})),
            );
        }
        if key != ADMIN_KEY {
            return (StatusCode::UNAUTHORIZED, Json(json!({"detail": "Admin key required."})));
        }
        let pr = body["bolt11"].as_str().unwrap();
        let hash = pr
            .parse::<lightning_invoice::Bolt11Invoice>()
            .unwrap()
            .payment_hash()
            .to_string();
        if hash == invoice(b"fail").1 {
            return (
                StatusCode::from_u16(520).unwrap(),
                Json(json!({"detail": "Payment failed: no route"})),
            );
        }
        let status = if hash == invoice(b"ok").1 { "success" } else { "pending" };
        (
            StatusCode::CREATED,
            Json(json!({"payment_hash": hash, "status": status, "preimage": "ab", "fee": -2500})),
        )
    }

    async fn status(State(fake): State<Fake>, Path(hash): Path<String>) -> (StatusCode, Json<Value>) {
        if hash == invoice(b"incoming").1 {
            return (
                StatusCode::OK,
                Json(json!({"paid": true, "preimage": "cd", "details": {"amount": 1_000_000, "fee": 0}})),
            );
        }
        if hash == invoice(b"slow").1 {
            let n = fake.status_checks.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                return (
                    StatusCode::OK,
                    Json(json!({"paid": false, "details": {"amount": -10_000, "fee": 0}})),
                );
            }
            return (
                StatusCode::OK,
                Json(json!({"paid": true, "preimage": "ef", "details": {"amount": -10_000, "fee": -1000}})),
            );
        }
        (
            StatusCode::NOT_FOUND,
            Json(json!({"detail": "Payment does not exist."})),
        )
    }

    async fn lnbits(admin_key: Option<&str>) -> LnbitsAdapter {
        let app = Router::new()
            .route("/api/v1/payments", post(payments))
            .route("/api/v1/payments/{hash}", get(status))
            .with_state(Fake::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        LnbitsAdapter::new(
            reqwest::Client::new(),
            LnbitsConfig {
                base_url: format!("http://{addr}"),
                invoice_key: INVOICE_KEY.into(),
                admin_key: admin_key.map(Into::into),
                webhook_url: None,
            },
        )
        .with_pay_polling(5, Duration::from_millis(10))
    }

    #[tokio::test]
    async fn creates_invoices_and_reads_status() {
        let ln = lnbits(None).await;
        let inv = ln.create_invoice(1_000, "test", 600).await.unwrap();
        assert!(inv.payment_request.starts_with("lnbcrt"));
        assert_eq!(
            ln.invoice_status(&inv.payment_hash).await.unwrap(),
            InvoiceStatus::Paid { amount_sats: 1_000 }
        );
        assert_eq!(ln.invoice_status(&"0".repeat(64)).await.unwrap(), InvoiceStatus::Failed);
    }

    #[tokio::test]
    async fn pays_immediately_or_after_pending() {
        let ln = lnbits(Some(ADMIN_KEY)).await;
        let paid = ln.pay_invoice(&invoice(b"ok").0, 100).await.unwrap();
        assert_eq!((paid.preimage.as_str(), paid.fee_sats), ("ab", 3));
        let paid = ln.pay_invoice(&invoice(b"slow").0, 100).await.unwrap();
        assert_eq!((paid.preimage.as_str(), paid.fee_sats), ("ef", 1));
    }

    #[tokio::test]
    async fn definite_failures_are_rejected() {
        let ln = lnbits(Some(ADMIN_KEY)).await;
        let err = ln.pay_invoice(&invoice(b"fail").0, 100).await.err().unwrap();
        assert!(
            matches!(&err, AdapterError::Rejected(m) if m.contains("no route")),
            "{err}"
        );
        let no_key = lnbits(None).await;
        assert!(matches!(
            no_key.pay_invoice(&invoice(b"ok").0, 100).await,
            Err(AdapterError::Rejected(_))
        ));
    }

    #[test]
    fn parses_double_encoded_webhooks() {
        let hash = "a".repeat(64);
        let inner = json!({"payment_hash": hash, "amount": 1000}).to_string();
        assert_eq!(
            webhook_payment_hash(json!(inner).to_string().as_bytes()),
            Some(hash.clone())
        );
        assert_eq!(webhook_payment_hash(inner.as_bytes()), Some(hash));
        assert_eq!(webhook_payment_hash(br#"{"payment_hash":"nothex"}"#), None);
    }
}
