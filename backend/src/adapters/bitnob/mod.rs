//! Bitnob API v2 (https://api.bitnob.com). Sandbox and production share the host; the client
//! secret selects the environment.
//!
//! - [`BitnobClient`]: HMAC-signed requests (`x-auth-*` headers).
//! - [`payout::BitnobPayoutAdapter`]: NGN bank transfers and mobile money payouts.
//! - [`rates::BitnobRateSource`]: the NGN/KES/XOF/XAF rate Bitnob actually pays out at.
//!
//! Payouts debit the workspace's Bitnob balance in `from_asset` (`source: offchain`), so that
//! balance must be pre-funded. Status arrives on `POST /api/v1/webhooks/bitnob`.

use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::Sha256;

use super::AdapterError;
use crate::services::payments::{PayoutEvent, PayoutEventStatus};

pub mod payout;
pub mod rates;

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
    /// Payout countries to enable (ISO alpha-2), e.g. `NG`, `KE`, `SN`. NG is a bank transfer;
    /// the others are mobile money.
    pub countries: Vec<String>,
}

pub struct BitnobClient {
    http: reqwest::Client,
    config: BitnobConfig,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
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

/// Per-country payout limits, in major units.
#[derive(Debug, Clone, Deserialize)]
pub struct Limit {
    pub country: String,
    pub currency: String,
    pub min_amount: String,
    pub max_amount: String,
}

#[derive(Debug, Deserialize)]
struct LimitsData {
    limits: Vec<Limit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Bank {
    pub bank_name: String,
    pub bank_code: String,
}

#[derive(Debug, Deserialize)]
struct BanksData {
    banks: Vec<Bank>,
}

#[derive(Debug, Deserialize)]
struct LookupData {
    account_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BitnobPayout {
    pub id: String,
    #[serde(default)]
    pub quote_id: Option<String>,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub sat_amount: Option<String>,
    #[serde(default)]
    pub btc_amount: Option<String>,
    #[serde(default)]
    pub settlement_amount: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PayoutData {
    payout: BitnobPayout,
}

#[derive(Debug, Deserialize)]
struct PayoutList {
    #[serde(default)]
    items: Vec<BitnobPayout>,
}

impl BitnobPayout {
    /// Sats Bitnob needs for this payout, from `sat_amount` or `btc_amount`.
    pub fn sats(&self) -> Option<i64> {
        if let Some(sats) = self.sat_amount.as_deref().and_then(|s| s.trim().parse::<i64>().ok()) {
            return Some(sats);
        }
        let btc: rust_decimal::Decimal = self.btc_amount.as_deref()?.trim().parse().ok()?;
        rust_decimal::prelude::ToPrimitive::to_i64(&(btc * rust_decimal::Decimal::from(100_000_000)).round())
    }
}

/// hex(HMAC-SHA256("{client_id}:{timestamp}:{nonce}:{body}", client_secret)).
pub fn request_signature(client_id: &str, client_secret: &str, timestamp: &str, nonce: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(client_secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(format!("{client_id}:{timestamp}:{nonce}:{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
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

impl BitnobClient {
    pub fn new(http: reqwest::Client, config: BitnobConfig) -> Self {
        Self { http, config }
    }

    pub fn config(&self) -> &BitnobConfig {
        &self.config
    }

    pub async fn call<T: DeserializeOwned>(
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

    pub async fn whoami(&self) -> Result<WhoAmI, AdapterError> {
        let identity: WhoAmI = self.call(Method::GET, "/api/whoami", &[], None).await?;
        if identity.active == Some(false) {
            return Err(AdapterError::Rejected("Bitnob workspace is not active".into()));
        }
        Ok(identity)
    }

    pub async fn limits(&self) -> Result<Vec<Limit>, AdapterError> {
        Ok(self
            .call::<LimitsData>(Method::GET, "/api/payouts/limits", &[], None)
            .await?
            .limits)
    }

    /// Banks or mobile money operators for a country.
    pub async fn banks(&self, country: &str) -> Result<Vec<Bank>, AdapterError> {
        let path = format!("/api/payouts/banks/{}", country.to_ascii_uppercase());
        Ok(self.call::<BanksData>(Method::GET, &path, &[], None).await?.banks)
    }

    /// Beneficiary field requirements for a country, as Bitnob describes them.
    pub async fn requirements(&self, country: &str) -> Result<Value, AdapterError> {
        let path = format!("/api/payouts/supported-countries/{}", country.to_ascii_uppercase());
        self.call(Method::GET, &path, &[], None).await
    }

    /// Account holder name; `None` when Bitnob can't resolve this account.
    pub async fn account_lookup(
        &self,
        country: &str,
        bank_code: &str,
        account_number: &str,
    ) -> Result<Option<String>, AdapterError> {
        let query = [
            ("country", country),
            ("bank_code", bank_code),
            ("account_number", account_number),
        ];
        match self
            .call::<LookupData>(Method::GET, "/api/payouts/account-lookup", &query, None)
            .await
        {
            Ok(data) if !data.account_name.trim().is_empty() => Ok(Some(data.account_name.trim().to_string())),
            Ok(_) => Ok(None),
            // Bitnob answers an unresolvable account with a 4xx.
            Err(AdapterError::Rejected(reason)) => {
                tracing::info!(%reason, "Bitnob account lookup rejected");
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    pub async fn quote(
        &self,
        currency: &str,
        country: &str,
        settlement_amount: &str,
        reason: &str,
        reference: &str,
    ) -> Result<BitnobPayout, AdapterError> {
        let body = json!({
            "from_asset": self.config.from_asset,
            "to_currency": currency,
            "source": "offchain",
            "country": country,
            "settlement_amount": settlement_amount,
            "payment_reason": reason,
            "reference": reference,
        });
        Ok(self
            .call::<PayoutData>(Method::POST, "/api/payouts/quotes", &[], Some(body))
            .await?
            .payout)
    }

    pub async fn initialize(
        &self,
        quote_id: &str,
        beneficiary: Value,
        reference: &str,
        reason: &str,
    ) -> Result<BitnobPayout, AdapterError> {
        let body = json!({
            "beneficiary": beneficiary,
            "reference": reference,
            "payment_reason": reason,
            "callback_url": self.config.callback_url,
        });
        let path = format!("/api/payouts/{quote_id}/initialize");
        Ok(self
            .call::<PayoutData>(Method::POST, &path, &[], Some(body))
            .await?
            .payout)
    }

    /// Commits the debit and releases the payout. Irreversible.
    pub async fn finalize(&self, quote_id: &str) -> Result<(), AdapterError> {
        let path = format!("/api/payouts/{quote_id}/finalize");
        self.call::<Value>(Method::POST, &path, &[], Some(json!({})))
            .await
            .map(|_| ())
    }

    /// Most recent payouts. Bitnob's list only filters by status, so callers match references.
    pub async fn recent_payouts(&self, limit: u32) -> Result<Vec<BitnobPayout>, AdapterError> {
        let limit = limit.to_string();
        Ok(self
            .call::<PayoutList>(Method::GET, "/api/payouts", &[("limit", &limit)], None)
            .await?
            .items)
    }

    pub async fn get_payout(&self, id: &str) -> Result<BitnobPayout, AdapterError> {
        Ok(self
            .call::<PayoutData>(Method::GET, &format!("/api/payouts/{id}"), &[], None)
            .await?
            .payout)
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

/// Maps a Bitnob payout status to a final outcome. Statuses seen in Bitnob's docs: QUOTE,
/// INITIATED, processing, success, expired. Others are treated as still in flight.
pub fn final_status(status: &str) -> Option<PayoutEventStatus> {
    match status.to_ascii_lowercase().as_str() {
        "success" | "successful" | "completed" => Some(PayoutEventStatus::Success),
        "expired" | "failed" | "cancelled" | "canceled" | "rejected" => Some(PayoutEventStatus::Failed),
        _ => None,
    }
}

/// True for statuses where Bitnob has not released any money: the quote or initialize step
/// happened but finalize did not, or the payout ended without paying.
pub fn never_paid(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "quote" | "initiated" | "initialized" | "expired" | "failed" | "cancelled" | "canceled" | "rejected"
    )
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake Bitnob that checks signatures like the real one and records calls.

    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    pub const CLIENT_ID: &str = "client-123";
    pub const SECRET: &str = "secret-xyz";

    #[derive(Clone, Default)]
    pub struct Fake {
        pub calls: Arc<Mutex<Vec<(String, Value)>>>,
        /// Payouts returned by the list endpoint.
        pub existing: Arc<Mutex<Vec<Value>>>,
    }

    type Reply = Result<Json<Value>, (StatusCode, Json<Value>)>;

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

    fn ok(data: Value) -> Reply {
        Ok(Json(json!({ "success": true, "data": data })))
    }

    async fn post_payouts(
        State(fake): State<Fake>,
        Path(rest): Path<String>,
        headers: HeaderMap,
        body: String,
    ) -> Reply {
        check_sig(&headers, &body)?;
        let parsed: Value = serde_json::from_str(&body).unwrap();
        fake.calls.lock().unwrap().push((rest.clone(), parsed.clone()));
        if rest.ends_with("finalize") {
            return ok(Value::Null);
        }
        // 1,000 sats per 1,000 units of settlement currency.
        let settlement: f64 = parsed["settlement_amount"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0);
        ok(json!({"payout": {
            "id": "bn-payout-1", "quote_id": "QT2_1", "status": "QUOTE",
            "sat_amount": format!("{}", settlement as i64), "reference": parsed["reference"],
        }}))
    }

    async fn get_any(
        State(fake): State<Fake>,
        Path(rest): Path<String>,
        headers: HeaderMap,
        Query(q): Query<std::collections::HashMap<String, String>>,
    ) -> Reply {
        check_sig(&headers, "")?;
        match rest.as_str() {
            "limits" => ok(json!({"limits": [
                {"country": "NG", "currency": "NGN", "min_amount": "500", "max_amount": "50000000"},
                {"country": "SN", "currency": "XOF", "min_amount": "100", "max_amount": "200000"},
                {"country": "CI", "currency": "XOF", "min_amount": "100", "max_amount": "2000000"},
            ]})),
            "banks/NG" => ok(json!({"banks": [{"bank_name": "Access bank", "bank_code": "000014"}]})),
            "banks/SN" | "banks/CI" => ok(json!({"banks": [{"bank_name": "Orange Money", "bank_code": "OM"}]})),
            "supported-countries/NG" => ok(json!({"code": "NG", "destination_types": {"bank": {"fields": [
                {"key": "bank_code", "required": true},
                {"key": "account_number", "required": true, "pattern": "^\\d{10}$"},
            ]}}})),
            "supported-countries/SN" | "supported-countries/CI" => {
                ok(json!({"code": "SN", "destination_types": {"mobile_money": {"fields": [
                    {"key": "bank_code", "required": true},
                    {"key": "account_number", "required": true, "pattern": "^\\d{9}$"},
                ]}}}))
            }
            "account-lookup" => {
                if q.get("account_number").map(String::as_str) == Some("0000000000") {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(json!({"status": 404, "detail": "account not found"})),
                    ));
                }
                if q.get("country").map(String::as_str) != Some("NG") {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({"status": 400, "detail": "lookup unsupported"})),
                    ));
                }
                ok(json!({"account_name": "ADA OBI", "is_verified": true}))
            }
            "" => ok(json!({"items": fake.existing.lock().unwrap().clone()})),
            id => ok(json!({"payout": {"id": id, "status": "success"}})),
        }
    }

    pub async fn serve(fake: Fake) -> String {
        let app = Router::new()
            .route(
                "/api/whoami",
                get(|headers: HeaderMap| async move {
                    check_sig(&headers, "")?;
                    ok(json!({"active": true, "client_name": "satsbridge", "environment": "sandbox"}))
                }),
            )
            .route(
                "/api/payouts",
                get(|State(fake): State<Fake>, headers: HeaderMap| async move {
                    check_sig(&headers, "")?;
                    ok(json!({"items": fake.existing.lock().unwrap().clone()}))
                }),
            )
            .route("/api/payouts/{*rest}", post(post_payouts).get(get_any))
            .with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    pub fn config(base_url: String, secret: &str, countries: &[&str]) -> BitnobConfig {
        BitnobConfig {
            base_url,
            client_id: CLIENT_ID.into(),
            client_secret: secret.into(),
            webhook_secret: "wh".into(),
            callback_url: "https://satsbridge.example/api/v1/webhooks/bitnob".into(),
            from_asset: "BTC".into(),
            countries: countries.iter().map(|c| c.to_string()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{fake::*, *};

    #[tokio::test]
    async fn whoami_reports_the_environment() {
        let base = serve(Fake::default()).await;
        let bitnob = BitnobClient::new(reqwest::Client::new(), config(base.clone(), SECRET, &["NG"]));
        assert_eq!(bitnob.whoami().await.unwrap().environment.as_deref(), Some("sandbox"));
        let wrong = BitnobClient::new(reqwest::Client::new(), config(base, "wrong-secret", &["NG"]));
        let err = wrong.whoami().await.err().unwrap();
        assert!(
            matches!(&err, AdapterError::Rejected(m) if m.contains("Invalid HMAC")),
            "{err}"
        );
    }

    #[test]
    fn maps_webhook_events_and_statuses() {
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
        assert!(matches!(final_status("SUCCESS"), Some(PayoutEventStatus::Success)));
        assert!(final_status("processing").is_none());
        assert!(never_paid("INITIATED") && never_paid("expired") && !never_paid("processing"));
    }
}
