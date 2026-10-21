//! Bitnob NGN bank payouts (https://api.bitnob.com, API v2).
//!
//! Flow per payout: `POST /api/payouts/quotes` -> `POST /api/payouts/{quote_id}/initialize`
//! -> `POST /api/payouts/{quote_id}/finalize` (irreversible). Funds are debited from the
//! workspace's Bitnob balance in `from_asset` (`source: offchain`), so that balance must be
//! pre-funded. Status arrives on `POST /api/v1/webhooks/bitnob`.
//!
//! Sandbox and production share the host; the client secret selects the environment.

use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::Sha256;

use super::{Institution, InstitutionKind, PayoutAccepted, PayoutAdapter, PayoutRequest, ResolvedRecipient};
use crate::{
    adapters::AdapterError,
    domain::{Currency, Rail, Recipient, money::minor_to_major},
    services::payments::{PayoutEvent, PayoutEventStatus},
};

pub const DEFAULT_BASE_URL: &str = "https://api.bitnob.com";
pub const SIGNATURE_HEADER: &str = "x-bitnob-signature";

#[derive(Clone)]
pub struct BitnobConfig {
    pub base_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub webhook_secret: String,
    /// Public URL of `/api/v1/webhooks/bitnob`, sent on every initialize.
    pub callback_url: String,
    /// Balance the payout is debited from: BTC, USDT or USDC.
    pub from_asset: String,
    pub min_payout_minor: i64,
    pub max_payout_minor: i64,
}

pub struct BitnobAdapter {
    http: reqwest::Client,
    config: BitnobConfig,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct PayoutData {
    payout: BitnobPayout,
}

#[derive(Deserialize)]
struct BitnobPayout {
    id: String,
    quote_id: String,
    #[serde(default)]
    sat_amount: Option<String>,
}

/// `GET /api/whoami`: confirms keys, clock and signature are all right.
#[derive(Debug, Deserialize)]
pub struct WhoAmI {
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub client_name: Option<String>,
    /// `sandbox` or `production`.
    #[serde(default)]
    pub environment: Option<String>,
}

#[derive(Deserialize)]
struct LookupData {
    account_name: String,
}

#[derive(Deserialize)]
struct BanksData {
    banks: Vec<Bank>,
}

#[derive(Deserialize)]
struct Bank {
    bank_name: String,
    bank_code: String,
}

/// RFC 7807 error body, or the `{"success": false, "error": {...}}` validation shape.
#[derive(Deserialize, Default)]
struct Problem {
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<ProblemInner>,
}

#[derive(Deserialize, Default)]
struct ProblemInner {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

/// hex(HMAC-SHA256("{client_id}:{timestamp}:{nonce}:{body}", client_secret)).
pub fn request_signature(client_id: &str, client_secret: &str, timestamp: &str, nonce: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(client_secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(format!("{client_id}:{timestamp}:{nonce}:{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

impl BitnobAdapter {
    pub fn new(http: reqwest::Client, config: BitnobConfig) -> Self {
        Self { http, config }
    }

    pub async fn whoami(&self) -> Result<WhoAmI, AdapterError> {
        let identity: WhoAmI = self.call(Method::GET, "/api/whoami", &[], None).await?;
        if identity.active == Some(false) {
            return Err(AdapterError::Rejected("Bitnob workspace is not active".into()));
        }
        Ok(identity)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<T, AdapterError> {
        // Sign exactly the bytes we send: compact JSON, or empty for GET.
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let timestamp = Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let signature = request_signature(
            &self.config.client_id,
            &self.config.client_secret,
            &timestamp,
            &nonce,
            &body,
        );

        let url = format!("{}{path}", self.config.base_url.trim_end_matches('/'));
        let mut req = self
            .http
            .request(method, url)
            .query(query)
            .header("x-auth-client", &self.config.client_id)
            .header("x-auth-timestamp", timestamp)
            .header("x-auth-nonce", nonce)
            .header("x-auth-signature", signature);
        if !body.is_empty() {
            req = req.header("content-type", "application/json").body(body);
        }

        let res = req
            .send()
            .await
            .map_err(|e| AdapterError::Unavailable(format!("Bitnob request failed: {e}")))?;
        let status = res.status();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| AdapterError::Unavailable(format!("Bitnob response failed: {e}")))?;
        if !status.is_success() {
            return Err(map_error(status, &bytes));
        }
        serde_json::from_slice::<Envelope<T>>(&bytes)
            .map(|e| e.data)
            .map_err(|e| AdapterError::Unavailable(format!("unexpected Bitnob response from {path}: {e}")))
    }
}

fn map_error(status: StatusCode, body: &[u8]) -> AdapterError {
    let problem: Problem = serde_json::from_slice(body).unwrap_or_default();
    let inner = problem.error.unwrap_or_default();
    let code = problem
        .code
        .or(inner.code)
        .unwrap_or_else(|| status.as_str().to_string());
    let detail = problem.detail.or(inner.message).unwrap_or_else(|| "no detail".into());
    let message = format!("Bitnob {} {code}: {detail}", status.as_u16());
    if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        AdapterError::Unavailable(message)
    } else {
        AdapterError::Rejected(message)
    }
}

fn bank_details(recipient: &Recipient) -> Result<(&str, &str), AdapterError> {
    match recipient {
        Recipient::NgBankTransfer {
            bank_code,
            account_number,
            ..
        } => {
            if !super::ngn_mock::is_valid_nuban(account_number) {
                return Err(AdapterError::InvalidRecipient(
                    "account number must be 10 digits".into(),
                ));
            }
            if bank_code.is_empty() || !bank_code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(AdapterError::InvalidRecipient("bank code must be numeric".into()));
            }
            Ok((bank_code, account_number))
        }
        _ => Err(AdapterError::InvalidRecipient(
            "expected an NG_BANK_TRANSFER recipient".into(),
        )),
    }
}

#[async_trait]
impl PayoutAdapter for BitnobAdapter {
    fn provider(&self) -> &'static str {
        "bitnob"
    }

    fn currency(&self) -> Currency {
        Currency::Ngn
    }

    fn rail(&self) -> Rail {
        Rail::NgBankTransfer
    }

    fn min_payout_minor(&self) -> Option<i64> {
        Some(self.config.min_payout_minor)
    }

    fn max_payout_minor(&self) -> Option<i64> {
        Some(self.config.max_payout_minor)
    }

    async fn institutions(&self) -> Result<Vec<Institution>, AdapterError> {
        let data: BanksData = self.call(Method::GET, "/api/payouts/banks/NG", &[], None).await?;
        Ok(data
            .banks
            .into_iter()
            .map(|b| Institution {
                code: b.bank_code,
                name: b.bank_name.trim().to_string(),
                kind: InstitutionKind::Bank,
            })
            .collect())
    }

    async fn resolve_recipient(&self, recipient: &Recipient) -> Result<ResolvedRecipient, AdapterError> {
        let (bank_code, account_number) = bank_details(recipient)?;
        let query = [
            ("country", "NG"),
            ("bank_code", bank_code),
            ("account_number", account_number),
        ];
        match self
            .call::<LookupData>(Method::GET, "/api/payouts/account-lookup", &query, None)
            .await
        {
            Ok(data) if !data.account_name.trim().is_empty() => Ok(ResolvedRecipient {
                account_name: Some(data.account_name.trim().to_string()),
                ..Default::default()
            }),
            Ok(_) => Err(AdapterError::RecipientNotFound),
            // Bitnob answers an unresolvable account with a 4xx.
            Err(AdapterError::Rejected(reason)) => {
                tracing::info!(%reason, "Bitnob account lookup rejected");
                Err(AdapterError::RecipientNotFound)
            }
            Err(err) => Err(err),
        }
    }

    async fn create_payout(&self, request: PayoutRequest<'_>) -> Result<PayoutAccepted, AdapterError> {
        let (bank_code, account_number) = bank_details(request.recipient)?;
        let account_name = request.account_name.ok_or_else(|| {
            AdapterError::InvalidRecipient("account name is required; resolve the account first".into())
        })?;
        let settlement_amount = minor_to_major(request.amount_minor, Currency::Ngn)
            .normalize()
            .to_string();
        let reference = request.idempotency_key;

        let quote: PayoutData = self
            .call(
                Method::POST,
                "/api/payouts/quotes",
                &[],
                Some(json!({
                    "from_asset": self.config.from_asset,
                    "to_currency": "NGN",
                    "source": "offchain",
                    "country": "NG",
                    "settlement_amount": settlement_amount,
                    "payment_reason": request.narration,
                    "reference": reference,
                })),
            )
            .await?;
        let quote_id = quote.payout.quote_id;
        if let Some(needed) = quote.payout.sat_amount.as_deref().and_then(|s| s.parse::<i64>().ok())
            && needed > request.amount_sats
        {
            // Customer was promised this amount; pay it and absorb the slippage.
            tracing::warn!(
                payment_id = reference,
                needed,
                budget = request.amount_sats,
                "Bitnob quote exceeds sat budget"
            );
        }

        let init: PayoutData = self
            .call(
                Method::POST,
                &format!("/api/payouts/{quote_id}/initialize"),
                &[],
                Some(json!({
                    "beneficiary": {
                        "destination_type": "bank",
                        "country": "NG",
                        "account_name": account_name,
                        "account_number": account_number,
                        "bank_code": bank_code,
                    },
                    "reference": reference,
                    "payment_reason": request.narration,
                    "callback_url": self.config.callback_url,
                })),
            )
            .await?;

        self.call::<Value>(
            Method::POST,
            &format!("/api/payouts/{quote_id}/finalize"),
            &[],
            Some(json!({})),
        )
        .await?;
        Ok(PayoutAccepted {
            provider_reference: init.payout.id,
            completed: false,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    pub event: String,
    pub event_id: String,
    pub data: WebhookData,
}

#[derive(Debug, Deserialize)]
pub struct WebhookData {
    /// Bitnob payout id; stored as our `provider_reference`.
    pub id: String,
}

/// Maps Bitnob payout events to our payout events. `initialized`/`processing` carry no state change.
pub fn to_payout_event(event: WebhookEvent) -> Option<PayoutEvent> {
    let (status, reason) = match event.event.as_str() {
        "payouts.withdrawal.success" => (PayoutEventStatus::Success, None),
        "payouts.withdrawal.expired" => (PayoutEventStatus::Failed, Some("payout expired at Bitnob".to_string())),
        _ => return None,
    };
    Some(PayoutEvent {
        event_id: event.event_id,
        provider_reference: event.data.id,
        status,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    const CLIENT_ID: &str = "client-123";
    const SECRET: &str = "secret-xyz";

    /// Records (path, body) of every signed call; rejects bad signatures like Bitnob does.
    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<(String, Value)>>>,
    }

    fn check_sig(headers: &HeaderMap, body: &str) -> Result<(), (StatusCode, Json<Value>)> {
        let h = |k: &str| {
            headers
                .get(k)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let expected = request_signature(
            &h("x-auth-client"),
            SECRET,
            &h("x-auth-timestamp"),
            &h("x-auth-nonce"),
            body,
        );
        if h("x-auth-client") != CLIENT_ID || h("x-auth-signature") != expected {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"status": 401, "code": "UNAUTHORIZED", "detail": "Invalid HMAC signature"})),
            ));
        }
        Ok(())
    }

    async fn post_any(
        State(fake): State<Fake>,
        Path(rest): Path<String>,
        headers: HeaderMap,
        body: String,
    ) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
        check_sig(&headers, &body)?;
        let parsed: Value = serde_json::from_str(&body).unwrap();
        fake.calls.lock().unwrap().push((rest.clone(), parsed));
        let payout =
            json!({"payout": {"id": "bn-payout-1", "quote_id": "QT2_1", "status": "QUOTE", "sat_amount": "1000"}});
        Ok(Json(
            json!({"success": true, "data": if rest.ends_with("finalize") { Value::Null } else { payout }}),
        ))
    }

    async fn lookup(
        headers: HeaderMap,
        Query(q): Query<std::collections::HashMap<String, String>>,
    ) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
        check_sig(&headers, "")?;
        if q["account_number"] == "0000000000" {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"status": 404, "code": "NOT_FOUND", "detail": "account not found"})),
            ));
        }
        Ok(Json(
            json!({"success": true, "data": {"account_name": "ADA OBI", "is_verified": true}}),
        ))
    }

    async fn adapter(secret: &str) -> (BitnobAdapter, Fake) {
        let fake = Fake::default();
        let app = Router::new()
            .route("/api/payouts/account-lookup", get(lookup))
            .route(
                "/api/whoami",
                get(|headers: HeaderMap| async move {
                    check_sig(&headers, "")?;
                    Ok::<_, (StatusCode, Json<Value>)>(Json(json!({
                        "success": true,
                        "data": {"active": true, "client_name": "satsbridge", "environment": "sandbox"}
                    })))
                }),
            )
            .route("/api/payouts/{*rest}", post(post_any))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = BitnobConfig {
            base_url: format!("http://{addr}"),
            client_id: CLIENT_ID.into(),
            client_secret: secret.into(),
            webhook_secret: "wh".into(),
            callback_url: "https://satsbridge.example/api/v1/webhooks/bitnob".into(),
            from_asset: "BTC".into(),
            min_payout_minor: 50_000,
            max_payout_minor: 5_000_000_000,
        };
        (BitnobAdapter::new(reqwest::Client::new(), config), fake)
    }

    fn recipient(account: &str) -> Recipient {
        Recipient::NgBankTransfer {
            bank_code: "000014".into(),
            account_number: account.into(),
            resolution_id: None,
        }
    }

    #[tokio::test]
    async fn whoami_reports_the_environment() {
        let (bitnob, _) = adapter(SECRET).await;
        assert_eq!(bitnob.whoami().await.unwrap().environment.as_deref(), Some("sandbox"));
        let (wrong, _) = adapter("wrong-secret").await;
        assert!(matches!(wrong.whoami().await, Err(AdapterError::Rejected(_))));
    }

    #[tokio::test]
    async fn resolves_account_names() {
        let (bitnob, _) = adapter(SECRET).await;
        let resolved = bitnob.resolve_recipient(&recipient("1421795566")).await.unwrap();
        assert_eq!(resolved.account_name.as_deref(), Some("ADA OBI"));
        assert!(matches!(
            bitnob.resolve_recipient(&recipient("0000000000")).await,
            Err(AdapterError::RecipientNotFound)
        ));
    }

    #[tokio::test]
    async fn quotes_initializes_and_finalizes() {
        let (bitnob, fake) = adapter(SECRET).await;
        let r = recipient("1421795566");
        let accepted = bitnob
            .create_payout(PayoutRequest {
                idempotency_key: "pay_1",
                recipient: &r,
                account_name: Some("ADA OBI"),
                amount_minor: 24_749_850,
                amount_sats: 164_999,
                currency: Currency::Ngn,
                narration: "SatsBridge pay_1".into(),
            })
            .await
            .unwrap();
        assert_eq!(accepted.provider_reference, "bn-payout-1");

        let calls = fake.calls.lock().unwrap();
        let paths: Vec<&str> = calls.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["quotes", "QT2_1/initialize", "QT2_1/finalize"]);
        assert_eq!(calls[0].1["settlement_amount"], "247498.5");
        assert_eq!(calls[0].1["from_asset"], "BTC");
        assert_eq!(calls[0].1["reference"], "pay_1");
        assert_eq!(calls[1].1["beneficiary"]["account_name"], "ADA OBI");
        assert_eq!(
            calls[1].1["callback_url"],
            "https://satsbridge.example/api/v1/webhooks/bitnob"
        );
    }

    #[tokio::test]
    async fn bad_credentials_are_rejected_not_retried() {
        let (bitnob, _) = adapter("wrong-secret").await;
        let r = recipient("1421795566");
        let err = bitnob
            .create_payout(PayoutRequest {
                idempotency_key: "pay_2",
                recipient: &r,
                account_name: Some("ADA OBI"),
                amount_minor: 100_000,
                amount_sats: 1_000,
                currency: Currency::Ngn,
                narration: "x".into(),
            })
            .await
            .err()
            .unwrap();
        assert!(
            matches!(&err, AdapterError::Rejected(m) if m.contains("Invalid HMAC")),
            "{err}"
        );
    }

    #[test]
    fn maps_webhook_events() {
        let parse = |event: &str| {
            to_payout_event(
                serde_json::from_value(json!({"event": event, "event_id": "e1", "data": {"id": "bn-1"}})).unwrap(),
            )
        };
        assert!(matches!(
            parse("payouts.withdrawal.success").unwrap().status,
            PayoutEventStatus::Success
        ));
        assert!(matches!(
            parse("payouts.withdrawal.expired").unwrap().status,
            PayoutEventStatus::Failed
        ));
        assert!(parse("payouts.processing").is_none());
    }
}
