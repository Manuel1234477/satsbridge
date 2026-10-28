//! Local stand-in for the Bitnob payout API, for developing without Bitnob credentials.
//!
//! It speaks the subset of Bitnob v2 that SatsBridge uses, checks the same HMAC request
//! signature, keeps a pretend balance, and sends signed `payouts.withdrawal.*` webhooks. It is
//! built from SatsBridge's reading of Bitnob's spec, so it proves our side of the integration and
//! nothing about the real service. Never point production at it.
//!
//! Point SatsBridge at it with `BITNOB_BASE_URL=http://127.0.0.1:9100` and use the same
//! `BITNOB_CLIENT_ID`, `BITNOB_CLIENT_SECRET` and `BITNOB_WEBHOOK_SECRET` on both sides.
//!
//! Settings (all optional):
//! - `SANDBOX_BIND` (127.0.0.1:9100)
//! - `SANDBOX_AUTO_COMPLETE_SECS`: seconds from finalize to the outcome webhook; 0 leaves payouts
//!   `processing` until `POST /sandbox/payouts/{id}/succeed|expire` (3)
//! - `SANDBOX_BALANCE_SATS`: starting balance (10,000,000)
//!
//! Magic values: an account number ending in `9999` makes the payout expire instead of succeed;
//! `0000000000` fails the NGN account lookup.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use chrono::Utc;
use hmac::{Hmac, Mac};
use regex::Regex;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use satsbridge::{adapters::bitnob::request_signature, security::constant_time_eq};
use serde_json::{Value, json};
use sha2::Sha512;

const MAX_CLOCK_SKEW_SECS: i64 = 300;
/// Added to the market price, so payouts cost slightly more than the mid rate like a real quote.
const SPREAD_BPS: i64 = 100;

struct Country {
    code: &'static str,
    currency: &'static str,
    /// `bank` or `mobile_money`.
    destination: &'static str,
    /// Major units.
    min: i64,
    max: i64,
    /// Major units per 1 BTC.
    btc_price: i64,
    number_pattern: &'static str,
    institutions: &'static [(&'static str, &'static str)],
}

const COUNTRIES: &[Country] = &[
    Country {
        code: "NG",
        currency: "NGN",
        destination: "bank",
        min: 500,
        max: 50_000_000,
        btc_price: 110_000_000,
        number_pattern: r"^\d{10}$",
        institutions: &[
            ("000014", "Access Bank"),
            ("000013", "Guaranty Trust Bank"),
            ("000015", "Zenith Bank"),
            ("000016", "First Bank of Nigeria"),
            ("000004", "United Bank for Africa"),
            ("000001", "Sterling Bank"),
            ("090267", "Kuda Microfinance Bank"),
            ("090405", "Moniepoint Microfinance Bank"),
            ("100004", "OPay"),
            ("100033", "PalmPay"),
        ],
    },
    Country {
        code: "KE",
        currency: "KES",
        destination: "mobile_money",
        min: 100,
        max: 250_000,
        btc_price: 10_700_000,
        number_pattern: r"^\d{9,12}$",
        institutions: &[("MPESA", "M-Pesa"), ("AIRTEL", "Airtel Money")],
    },
    Country {
        code: "SN",
        currency: "XOF",
        destination: "mobile_money",
        min: 100,
        max: 2_000_000,
        btc_price: 48_300_000,
        number_pattern: r"^\d{9}$",
        institutions: &[("OM", "Orange Money"), ("WAVE", "Wave")],
    },
    Country {
        code: "CI",
        currency: "XOF",
        destination: "mobile_money",
        min: 100,
        max: 2_000_000,
        btc_price: 48_300_000,
        number_pattern: r"^\d{10}$",
        institutions: &[("OM", "Orange Money"), ("MTN", "MTN MoMo"), ("WAVE", "Wave")],
    },
    Country {
        code: "NE",
        currency: "XOF",
        destination: "mobile_money",
        min: 100,
        max: 1_000_000,
        btc_price: 48_300_000,
        number_pattern: r"^\d{8}$",
        institutions: &[("AIRTEL", "Airtel Money"), ("MOOV", "Moov Money")],
    },
    Country {
        code: "CM",
        currency: "XAF",
        destination: "mobile_money",
        min: 100,
        max: 2_000_000,
        btc_price: 48_300_000,
        number_pattern: r"^\d{9}$",
        institutions: &[("MTN", "MTN MoMo"), ("OM", "Orange Money")],
    },
];

const NAMES: &[&str] = &["ADA OBI", "CHUKWUEMEKA OKAFOR", "AMINA YUSUF", "FATOU DIALLO", "JOHN KAMAU"];

fn country(code: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.code.eq_ignore_ascii_case(code))
}

struct Settings {
    client_id: String,
    client_secret: String,
    webhook_secret: String,
    auto_complete_secs: u64,
}

#[derive(Clone)]
struct Payout {
    id: String,
    quote_id: String,
    reference: String,
    status: String,
    country: &'static Country,
    settlement: Decimal,
    sats: i64,
    account_number: Option<String>,
    callback_url: Option<String>,
    created_at: String,
}

impl Payout {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "quote_id": self.quote_id,
            "reference": self.reference,
            "status": self.status,
            "country": self.country.code,
            "to_currency": self.country.currency,
            "settlement_amount": self.settlement.normalize().to_string(),
            "sat_amount": self.sats.to_string(),
            "created_at": self.created_at,
        })
    }
}

struct Inner {
    payouts: Vec<Payout>,
    balance_sats: i64,
}

struct Sandbox {
    settings: Settings,
    http: reqwest::Client,
    inner: Mutex<Inner>,
}

type App = Arc<Sandbox>;
type ApiError = (StatusCode, Json<Value>);
type Reply = Result<Json<Value>, ApiError>;

fn fail(status: StatusCode, code: &str, detail: impl Into<String>) -> ApiError {
    (
        status,
        Json(json!({ "status": status.as_u16(), "code": code, "detail": detail.into() })),
    )
}

fn bad_request(detail: impl Into<String>) -> ApiError {
    fail(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", detail)
}

fn not_found(detail: impl Into<String>) -> ApiError {
    fail(StatusCode::NOT_FOUND, "NOT_FOUND", detail)
}

fn ok(data: Value) -> Reply {
    Ok(Json(json!({ "success": true, "data": data })))
}

/// Checks the `x-auth-*` headers the way Bitnob does: client id, clock, and the HMAC of the body.
fn authenticate(app: &Sandbox, headers: &HeaderMap, body: &str) -> Result<(), ApiError> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let unauthorized = |detail: &str| fail(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", detail);
    let timestamp: i64 = header("x-auth-timestamp")
        .parse()
        .map_err(|_| unauthorized("missing or invalid x-auth-timestamp"))?;
    if (Utc::now().timestamp() - timestamp).abs() > MAX_CLOCK_SKEW_SECS {
        return Err(unauthorized("timestamp outside the allowed window; check the system clock"));
    }
    let expected = request_signature(
        &app.settings.client_id,
        &app.settings.client_secret,
        header("x-auth-timestamp"),
        header("x-auth-nonce"),
        body,
    );
    let client_ok = constant_time_eq(header("x-auth-client").as_bytes(), app.settings.client_id.as_bytes());
    let signature_ok = constant_time_eq(header("x-auth-signature").as_bytes(), expected.as_bytes());
    if !(client_ok && signature_ok) {
        return Err(unauthorized("Invalid HMAC signature"));
    }
    Ok(())
}

fn sats_for(country: &Country, settlement: Decimal) -> Option<i64> {
    let spread = Decimal::from(10_000 + SPREAD_BPS) / Decimal::from(10_000);
    (settlement * Decimal::from(100_000_000) / Decimal::from(country.btc_price) * spread)
        .ceil()
        .to_i64()
}

fn requirements(c: &Country) -> Value {
    json!({
        "code": c.code,
        "destination_types": { c.destination: { "fields": [
            { "key": "bank_code", "required": true },
            { "key": "account_number", "required": true, "pattern": c.number_pattern },
            { "key": "account_name", "required": true },
        ]}},
    })
}

async fn whoami(State(app): State<App>, headers: HeaderMap) -> Reply {
    authenticate(&app, &headers, "")?;
    ok(json!({ "active": true, "client_name": "satsbridge (local sandbox)", "environment": "sandbox" }))
}

async fn list(State(app): State<App>, headers: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Reply {
    authenticate(&app, &headers, "")?;
    let limit = q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50usize);
    let inner = app.inner.lock().unwrap();
    let items: Vec<Value> = inner.payouts.iter().rev().take(limit).map(Payout::json).collect();
    ok(json!({ "items": items }))
}

/// `GET /api/payouts/{rest}`: limits, directories, requirements, lookup, or one payout.
async fn get_payouts(
    State(app): State<App>,
    Path(rest): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Reply {
    authenticate(&app, &headers, "")?;
    match rest.as_str() {
        "limits" => ok(json!({ "limits": COUNTRIES.iter().map(|c| json!({
            "country": c.code,
            "currency": c.currency,
            "min_amount": c.min.to_string(),
            "max_amount": c.max.to_string(),
        })).collect::<Vec<_>>() })),
        "account-lookup" => {
            let c = q.get("country").and_then(|c| country(c)).ok_or_else(|| bad_request("unknown country"))?;
            if c.destination != "bank" {
                return Err(bad_request("account lookup is only available for bank countries"));
            }
            let number = q.get("account_number").map(String::as_str).unwrap_or_default();
            let bank = q.get("bank_code").map(String::as_str).unwrap_or_default();
            if !c.institutions.iter().any(|(code, _)| *code == bank) {
                return Err(bad_request("unknown bank_code"));
            }
            if number == "0000000000" {
                return Err(not_found("account not found"));
            }
            let sum: usize = number.bytes().map(|b| b as usize).sum();
            ok(json!({ "account_name": NAMES[sum % NAMES.len()], "is_verified": true }))
        }
        other => {
            if let Some(code) = other.strip_prefix("banks/") {
                let c = country(code).ok_or_else(|| not_found("unknown country"))?;
                return ok(json!({ "banks": c.institutions.iter()
                    .map(|(code, name)| json!({ "bank_name": name, "bank_code": code }))
                    .collect::<Vec<_>>() }));
            }
            if let Some(code) = other.strip_prefix("supported-countries/") {
                let c = country(code).ok_or_else(|| not_found("unknown country"))?;
                return ok(requirements(c));
            }
            let inner = app.inner.lock().unwrap();
            inner
                .payouts
                .iter()
                .find(|p| p.id == other || p.quote_id == other)
                .map(|p| json!({ "payout": p.json() }))
                .map(|v| Json(json!({ "success": true, "data": v })))
                .ok_or_else(|| not_found("payout not found"))
        }
    }
}

fn parse_body(body: &str) -> Result<Value, ApiError> {
    serde_json::from_str(body).map_err(|e| bad_request(format!("invalid JSON body: {e}")))
}

fn text<'a>(body: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    body[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| bad_request(format!("{key} is required")))
}

fn new_quote(app: &Sandbox, body: &Value) -> Reply {
    if !["BTC", "USDT", "USDC"].contains(&text(body, "from_asset")?) {
        return Err(bad_request("from_asset must be BTC, USDT or USDC"));
    }
    if text(body, "source")? != "offchain" {
        return Err(bad_request("only source \"offchain\" is supported"));
    }
    let c = country(text(body, "country")?).ok_or_else(|| bad_request("unsupported country"))?;
    if !text(body, "to_currency")?.eq_ignore_ascii_case(c.currency) {
        return Err(bad_request(format!("{} pays out in {}", c.code, c.currency)));
    }
    let settlement: Decimal = text(body, "settlement_amount")?
        .parse()
        .map_err(|_| bad_request("settlement_amount must be a decimal string"))?;
    if settlement < Decimal::from(c.min) || settlement > Decimal::from(c.max) {
        return Err(bad_request(format!(
            "settlement_amount must be between {} and {} {}",
            c.min, c.max, c.currency
        )));
    }
    let sats = sats_for(c, settlement).ok_or_else(|| bad_request("settlement_amount is too large"))?;
    let payout = Payout {
        id: format!("bn_{}", uuid::Uuid::new_v4().simple()),
        quote_id: format!("QT2_{}", uuid::Uuid::new_v4().simple()),
        reference: text(body, "reference")?.to_string(),
        status: "QUOTE".into(),
        country: c,
        settlement,
        sats,
        account_number: None,
        callback_url: None,
        created_at: Utc::now().to_rfc3339(),
    };
    let data = json!({ "payout": payout.json() });
    app.inner.lock().unwrap().payouts.push(payout);
    ok(data)
}

fn initialize(app: &Sandbox, quote_id: &str, body: &Value) -> Reply {
    let mut inner = app.inner.lock().unwrap();
    let payout = inner
        .payouts
        .iter_mut()
        .find(|p| p.quote_id == quote_id)
        .ok_or_else(|| not_found("quote not found"))?;
    if payout.status != "QUOTE" {
        return Err(fail(
            StatusCode::CONFLICT,
            "INVALID_STATE",
            format!("payout is {}", payout.status),
        ));
    }
    let b = &body["beneficiary"];
    let c = payout.country;
    if text(b, "destination_type")? != c.destination {
        return Err(bad_request(format!("destination_type must be {}", c.destination)));
    }
    if !text(b, "country")?.eq_ignore_ascii_case(c.code) {
        return Err(bad_request("beneficiary country does not match the quote"));
    }
    text(b, "account_name")?;
    let bank = text(b, "bank_code")?;
    if !c.institutions.iter().any(|(code, _)| *code == bank) {
        return Err(bad_request(format!("unknown bank_code {bank}")));
    }
    let number = text(b, "account_number")?;
    if !Regex::new(c.number_pattern).expect("static pattern").is_match(number) {
        return Err(bad_request(format!("account_number must match {}", c.number_pattern)));
    }
    payout.status = "INITIATED".into();
    payout.account_number = Some(number.to_string());
    payout.callback_url = body["callback_url"].as_str().map(str::to_string);
    if let Some(reference) = body["reference"].as_str() {
        payout.reference = reference.to_string();
    }
    ok(json!({ "payout": payout.json() }))
}

fn finalize(app: &App, quote_id: &str) -> Reply {
    let mut inner = app.inner.lock().unwrap();
    let balance = inner.balance_sats;
    let payout = inner
        .payouts
        .iter_mut()
        .find(|p| p.quote_id == quote_id)
        .ok_or_else(|| not_found("quote not found"))?;
    if payout.status != "INITIATED" {
        return Err(fail(
            StatusCode::CONFLICT,
            "INVALID_STATE",
            format!("payout is {}", payout.status),
        ));
    }
    if payout.sats > balance {
        return Err(bad_request(format!(
            "insufficient balance: payout needs {} sats, balance is {balance}",
            payout.sats
        )));
    }
    payout.status = "processing".into();
    let (id, sats) = (payout.id.clone(), payout.sats);
    inner.balance_sats -= sats;
    drop(inner);

    let secs = app.settings.auto_complete_secs;
    if secs > 0 {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let fails = {
                let inner = app.inner.lock().unwrap();
                inner
                    .payouts
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| p.account_number.as_deref())
                    .is_some_and(|n| n.ends_with("9999"))
            };
            let _ = complete(&app, &id, !fails).await;
        });
    }
    ok(Value::Null)
}

/// Moves a `processing` payout to its final status and sends the webhook. An expired payout
/// returns its sats to the balance.
async fn complete(app: &App, payout_id: &str, success: bool) -> Result<Value, ApiError> {
    let payout = {
        let mut inner = app.inner.lock().unwrap();
        let payout = inner
            .payouts
            .iter_mut()
            .find(|p| p.id == payout_id)
            .ok_or_else(|| not_found("payout not found"))?;
        if payout.status != "processing" {
            return Err(fail(
                StatusCode::CONFLICT,
                "INVALID_STATE",
                format!("payout is {}", payout.status),
            ));
        }
        payout.status = if success { "success" } else { "expired" }.into();
        let payout = payout.clone();
        if !success {
            inner.balance_sats += payout.sats;
        }
        payout
    };
    let event = if success {
        "payouts.withdrawal.success"
    } else {
        "payouts.withdrawal.expired"
    };
    let app = app.clone();
    let delivered = payout.clone();
    tokio::spawn(async move { deliver(&app, &delivered, event).await });
    Ok(payout.json())
}

/// Signs the body with HMAC-SHA512 and retries until the receiver answers 2xx, like Bitnob.
/// Every attempt carries the same `event_id`.
async fn deliver(app: &Sandbox, payout: &Payout, event: &str) {
    let Some(url) = payout.callback_url.as_deref() else {
        tracing_log(&format!("{event} for {}: no callback_url, not sent", payout.id));
        return;
    };
    let body = json!({
        "event": event,
        "event_id": format!("evt_{}", uuid::Uuid::new_v4().simple()),
        "data": { "id": payout.id, "quote_id": payout.quote_id, "reference": payout.reference, "status": payout.status },
    })
    .to_string();
    let mut mac = Hmac::<Sha512>::new_from_slice(app.settings.webhook_secret.as_bytes()).expect("any key length");
    mac.update(body.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    for attempt in 1..=5u64 {
        let sent = app
            .http
            .post(url)
            .header("content-type", "application/json")
            .header("x-bitnob-signature", &signature)
            .body(body.clone())
            .send()
            .await;
        match sent {
            Ok(res) if res.status().is_success() => {
                tracing_log(&format!("{event} for {} delivered (attempt {attempt})", payout.id));
                return;
            }
            Ok(res) => tracing_log(&format!(
                "{event} for {}: receiver answered {} (attempt {attempt})",
                payout.id,
                res.status()
            )),
            Err(err) => tracing_log(&format!("{event} for {}: {err} (attempt {attempt})", payout.id)),
        }
        tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
    }
    tracing_log(&format!("{event} for {}: gave up after 5 attempts", payout.id));
}

fn tracing_log(message: &str) {
    eprintln!("{} {message}", Utc::now().format("%H:%M:%S"));
}

async fn post_payouts(State(app): State<App>, Path(rest): Path<String>, headers: HeaderMap, body: String) -> Reply {
    authenticate(&app, &headers, &body)?;
    let parsed = if body.is_empty() { Value::Null } else { parse_body(&body)? };
    match rest.as_str() {
        "quotes" => new_quote(&app, &parsed),
        other => match other.split_once('/') {
            Some((quote_id, "initialize")) => initialize(&app, quote_id, &parsed),
            Some((quote_id, "finalize")) => finalize(&app, quote_id),
            _ => Err(not_found("no such endpoint")),
        },
    }
}

// --- Sandbox controls: not part of Bitnob's API, unauthenticated, localhost only. ---

async fn state(State(app): State<App>) -> Json<Value> {
    let inner = app.inner.lock().unwrap();
    Json(json!({
        "balance_sats": inner.balance_sats,
        "payouts": inner.payouts.iter().map(Payout::json).collect::<Vec<_>>(),
    }))
}

async fn force(State(app): State<App>, Path((id, outcome)): Path<(String, String)>) -> Reply {
    let success = match outcome.as_str() {
        "succeed" => true,
        "expire" => false,
        _ => return Err(not_found("use succeed or expire")),
    };
    Ok(Json(complete(&app, &id, success).await?))
}

async fn fund(State(app): State<App>, Json(body): Json<Value>) -> Reply {
    let sats = body["sats"].as_i64().ok_or_else(|| bad_request("sats is required"))?;
    let mut inner = app.inner.lock().unwrap();
    inner.balance_sats += sats;
    Ok(Json(json!({ "balance_sats": inner.balance_sats })))
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let bind: SocketAddr = env("SANDBOX_BIND").unwrap_or_else(|| "127.0.0.1:9100".into()).parse()?;
    let client_id = env("BITNOB_CLIENT_ID").unwrap_or_else(|| "sandbox-client".into());
    let client_secret = env("BITNOB_CLIENT_SECRET").unwrap_or_else(|| {
        eprintln!("BITNOB_CLIENT_SECRET is not set; using \"sandbox-secret\". Set it to the same value in SatsBridge's .env.");
        "sandbox-secret".into()
    });
    let webhook_secret = env("BITNOB_WEBHOOK_SECRET").unwrap_or_else(|| {
        eprintln!("BITNOB_WEBHOOK_SECRET is not set; using \"sandbox-webhook-secret\". Set it to the same value in SatsBridge's .env.");
        "sandbox-webhook-secret".into()
    });
    let settings = Settings {
        client_id,
        client_secret,
        webhook_secret,
        auto_complete_secs: env("SANDBOX_AUTO_COMPLETE_SECS").and_then(|v| v.parse().ok()).unwrap_or(3),
    };
    let balance_sats = env("SANDBOX_BALANCE_SATS").and_then(|v| v.parse().ok()).unwrap_or(10_000_000);
    let app: App = Arc::new(Sandbox {
        settings,
        http: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?,
        inner: Mutex::new(Inner {
            payouts: Vec::new(),
            balance_sats,
        }),
    });

    let router = Router::new()
        .route("/api/whoami", get(whoami))
        .route("/api/payouts", get(list))
        .route("/api/payouts/{*rest}", get(get_payouts).post(post_payouts))
        .route("/sandbox/state", get(state))
        .route("/sandbox/fund", post(fund))
        .route("/sandbox/payouts/{id}/{outcome}", post(force))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("Bitnob sandbox stand-in listening on http://{bind}  (balance {balance_sats} sats)");
    axum::serve(listener, router).await?;
    Ok(())
}
