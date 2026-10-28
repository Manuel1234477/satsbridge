//! Local stand-in for the LNbits HTTP API, for running SatsBridge without a Lightning node.
//!
//! It implements the endpoints SatsBridge uses (`/api/v1/wallet`, `/api/v1/payments`) with the
//! same key rules as LNbits: an invoice key creates and reads invoices, an admin key also spends.
//! Invoices are real signed (regtest) BOLT11 invoices, so SatsBridge's own decoding and checks run
//! unchanged. Nothing here touches a real network or moves real money.
//!
//! SatsBridge still settles a payment only when this server reports the invoice paid. Playing the
//! sender's wallet is done here, on a separate page: open `/sandbox` and press **Pay** next to an
//! invoice, or `POST /sandbox/pay/{payment_hash}`. Outgoing payments (refunds, Tapnob payouts)
//! succeed at once and are debited from the pretend balance.
//!
//! Point SatsBridge at it with `LNBITS_URL=http://127.0.0.1:5000` and the two keys printed at
//! startup (or set `LNBITS_INVOICE_KEY` / `LNBITS_ADMIN_KEY` for both programs).
//!
//! Settings (all optional): `LNBITS_SANDBOX_BIND` (127.0.0.1:5000), `LNBITS_INVOICE_KEY`,
//! `LNBITS_ADMIN_KEY`, `SANDBOX_LN_BALANCE_SATS` (1,000,000).

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
};
use bitcoin::{
    hashes::{Hash, sha256},
    secp256k1::{Secp256k1, SecretKey},
};
use chrono::Utc;
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use satsbridge::adapters::lightning::decode_invoice;
use serde_json::{Value, json};

/// Fee charged on outgoing payments, in millisats.
const OUTGOING_FEE_MSAT: i64 = 1_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Invoice,
    Admin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    In,
    Out,
}

#[derive(Clone)]
struct Record {
    hash: String,
    bolt11: String,
    direction: Direction,
    amount_msat: i64,
    fee_msat: i64,
    memo: String,
    paid: bool,
    preimage: String,
    created_at: i64,
    expires_at: i64,
    webhook: Option<String>,
}

impl Record {
    fn expired(&self) -> bool {
        !self.paid && self.expires_at <= Utc::now().timestamp()
    }

    fn json(&self) -> Value {
        json!({
            "payment_hash": self.hash,
            "payment_request": self.bolt11,
            "direction": if self.direction == Direction::In { "in" } else { "out" },
            "amount_sats": self.amount_msat / 1000,
            "memo": self.memo,
            "status": if self.paid { "paid" } else if self.expired() { "expired" } else { "pending" },
            "created_at": self.created_at,
            "expires_at": self.expires_at,
        })
    }
}

struct Inner {
    records: Vec<Record>,
    balance_msat: i64,
}

struct Sandbox {
    invoice_key: String,
    admin_key: String,
    http: reqwest::Client,
    inner: Mutex<Inner>,
}

type App = Arc<Sandbox>;
type Reply = (StatusCode, Json<Value>);

fn detail(status: StatusCode, message: &str) -> Reply {
    (status, Json(json!({ "detail": message })))
}

fn role(app: &Sandbox, headers: &HeaderMap) -> Option<Role> {
    let key = headers.get("x-api-key")?.to_str().ok()?;
    if key == app.admin_key {
        Some(Role::Admin)
    } else if key == app.invoice_key {
        Some(Role::Invoice)
    } else {
        None
    }
}

fn random32() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes
}

/// Returns (bolt11, payment hash, preimage).
fn make_invoice(amount_msat: u64, memo: &str, unhashed_description: Option<&[u8]>, expiry_secs: u64) -> (String, String, String) {
    let key = SecretKey::from_slice(&[7; 32]).expect("valid key");
    let preimage = random32();
    let hash = sha256::Hash::hash(&preimage);
    let builder = InvoiceBuilder::new(Currency::Regtest);
    let builder = match unhashed_description {
        Some(metadata) => builder.description_hash(sha256::Hash::hash(metadata)).payment_hash(hash),
        None => builder.description(memo.to_string()).payment_hash(hash),
    };
    let bolt11 = builder
        .payment_secret(PaymentSecret(random32()))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .expiry_time(Duration::from_secs(expiry_secs))
        .amount_milli_satoshis(amount_msat)
        .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .expect("signable invoice")
        .to_string();
    (bolt11, hash.to_string(), hex::encode(preimage))
}

async fn wallet(State(app): State<App>, headers: HeaderMap) -> Reply {
    let Some(role) = role(&app, &headers) else {
        return detail(StatusCode::UNAUTHORIZED, "Invalid key.");
    };
    let balance = app.inner.lock().unwrap().balance_msat;
    let mut body = json!({ "name": "SatsBridge sandbox wallet", "balance": balance });
    // Like LNbits, only an admin key is shown the wallet id.
    if role == Role::Admin {
        body["id"] = json!("sandbox-wallet");
    }
    (StatusCode::OK, Json(body))
}

async fn create_payment(State(app): State<App>, headers: HeaderMap, Json(body): Json<Value>) -> Reply {
    let Some(role) = role(&app, &headers) else {
        return detail(StatusCode::UNAUTHORIZED, "Invalid key.");
    };
    if body["out"].as_bool().unwrap_or(false) {
        if role != Role::Admin {
            return detail(StatusCode::UNAUTHORIZED, "Admin key required.");
        }
        return send(&app, body["bolt11"].as_str().unwrap_or_default());
    }
    receive(&app, &body)
}

fn receive(app: &App, body: &Value) -> Reply {
    let Some(sats) = body["amount"].as_i64().filter(|a| *a > 0) else {
        return detail(StatusCode::BAD_REQUEST, "amount must be a positive number of sats.");
    };
    let memo = body["memo"].as_str().unwrap_or_default();
    let unhashed = match body["unhashed_description"].as_str() {
        Some(h) => match hex::decode(h) {
            Ok(bytes) => Some(bytes),
            Err(_) => return detail(StatusCode::BAD_REQUEST, "unhashed_description must be hex."),
        },
        None => None,
    };
    let expiry = body["expiry"].as_u64().filter(|e| *e > 0).unwrap_or(3600);
    let (bolt11, hash, preimage) = make_invoice(sats as u64 * 1000, memo, unhashed.as_deref(), expiry);
    let now = Utc::now().timestamp();
    app.inner.lock().unwrap().records.push(Record {
        hash: hash.clone(),
        bolt11: bolt11.clone(),
        direction: Direction::In,
        amount_msat: sats * 1000,
        fee_msat: 0,
        memo: memo.to_string(),
        paid: false,
        preimage,
        created_at: now,
        expires_at: now + expiry as i64,
        webhook: body["webhook"].as_str().map(str::to_string),
    });
    eprintln!("{} invoice {} for {sats} sats", Utc::now().format("%H:%M:%S"), &hash[..12]);
    (
        StatusCode::CREATED,
        Json(json!({
            "payment_hash": hash,
            "payment_request": bolt11,
            "bolt11": bolt11,
            "checking_id": hash,
            "status": "pending",
        })),
    )
}

fn send(app: &App, bolt11: &str) -> Reply {
    let Ok(decoded) = decode_invoice(bolt11) else {
        return detail(StatusCode::BAD_REQUEST, "Invalid BOLT11 invoice.");
    };
    let Some(amount_msat) = decoded.amount_msat.map(|a| a as i64) else {
        return detail(StatusCode::BAD_REQUEST, "Invoices without an amount are not supported.");
    };
    let mut inner = app.inner.lock().unwrap();
    if inner.records.iter().any(|r| r.hash == decoded.payment_hash && r.direction == Direction::Out) {
        return detail(StatusCode::BAD_REQUEST, "Payment already paid.");
    }
    // Paying one of our own invoices moves nothing but marks it paid.
    if let Some(own) = inner
        .records
        .iter_mut()
        .find(|r| r.hash == decoded.payment_hash && r.direction == Direction::In)
    {
        if own.paid {
            return detail(StatusCode::BAD_REQUEST, "Payment already paid.");
        }
        own.paid = true;
        let (hash, preimage, webhook, amount) = (own.hash.clone(), own.preimage.clone(), own.webhook.clone(), own.amount_msat);
        drop(inner);
        notify(app, webhook, &hash, amount);
        return (
            StatusCode::CREATED,
            Json(json!({ "payment_hash": hash, "status": "success", "preimage": preimage, "fee": 0 })),
        );
    }
    if inner.balance_msat < amount_msat + OUTGOING_FEE_MSAT {
        return detail(StatusCode::BAD_REQUEST, "Insufficient balance.");
    }
    inner.balance_msat -= amount_msat + OUTGOING_FEE_MSAT;
    let preimage = hex::encode(random32());
    let now = Utc::now().timestamp();
    inner.records.push(Record {
        hash: decoded.payment_hash.clone(),
        bolt11: bolt11.trim().to_string(),
        direction: Direction::Out,
        amount_msat,
        fee_msat: OUTGOING_FEE_MSAT,
        memo: String::new(),
        paid: true,
        preimage: preimage.clone(),
        created_at: now,
        expires_at: now,
        webhook: None,
    });
    eprintln!(
        "{} sent {} sats (invoice {})",
        Utc::now().format("%H:%M:%S"),
        amount_msat / 1000,
        &decoded.payment_hash[..12]
    );
    (
        StatusCode::CREATED,
        Json(json!({
            "payment_hash": decoded.payment_hash,
            "status": "success",
            "preimage": preimage,
            "fee": -OUTGOING_FEE_MSAT,
        })),
    )
}

async fn payment_status(State(app): State<App>, headers: HeaderMap, Path(hash): Path<String>) -> Reply {
    if role(&app, &headers).is_none() {
        return detail(StatusCode::UNAUTHORIZED, "Invalid key.");
    }
    let inner = app.inner.lock().unwrap();
    let Some(r) = inner.records.iter().find(|r| r.hash == hash) else {
        return detail(StatusCode::NOT_FOUND, "Payment does not exist.");
    };
    let sign = if r.direction == Direction::In { 1 } else { -1 };
    let status = if r.paid {
        "success"
    } else if r.expired() {
        "failed"
    } else {
        "pending"
    };
    let mut body = json!({
        "paid": r.paid,
        "status": status,
        "details": { "amount": sign * r.amount_msat, "fee": -r.fee_msat, "payment_hash": r.hash, "memo": r.memo },
    });
    if r.paid {
        body["preimage"] = json!(r.preimage);
    }
    (StatusCode::OK, Json(body))
}

/// LNbits posts the payment to the invoice's webhook as a JSON-encoded string.
fn notify(app: &App, webhook: Option<String>, hash: &str, amount_msat: i64) {
    let Some(url) = webhook else { return };
    let body = Value::String(json!({ "payment_hash": hash, "amount": amount_msat, "pending": false }).to_string()).to_string();
    let http = app.http.clone();
    tokio::spawn(async move {
        for attempt in 1..=3u64 {
            let sent = http
                .post(&url)
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await;
            if sent.is_ok_and(|r| r.status().is_success()) {
                return;
            }
            tokio::time::sleep(Duration::from_secs(attempt)).await;
        }
    });
}

// --- Sandbox controls: not part of LNbits, unauthenticated, localhost only. ---

fn mark_paid(app: &App, hash: &str) -> Result<i64, &'static str> {
    let mut inner = app.inner.lock().unwrap();
    let Some(r) = inner
        .records
        .iter_mut()
        .find(|r| r.hash == hash && r.direction == Direction::In)
    else {
        return Err("no such invoice");
    };
    if r.paid {
        return Err("already paid");
    }
    if r.expired() {
        return Err("invoice expired");
    }
    r.paid = true;
    let (webhook, amount) = (r.webhook.clone(), r.amount_msat);
    inner.balance_msat += amount;
    drop(inner);
    eprintln!("{} invoice {} paid ({} sats)", Utc::now().format("%H:%M:%S"), &hash[..12], amount / 1000);
    notify(app, webhook, hash, amount);
    Ok(amount / 1000)
}

async fn sandbox_pay(State(app): State<App>, Path(hash): Path<String>) -> Reply {
    match mark_paid(&app, &hash) {
        Ok(sats) => (StatusCode::OK, Json(json!({ "paid": true, "amount_sats": sats }))),
        Err(reason) => detail(StatusCode::CONFLICT, reason),
    }
}

async fn sandbox_fund(State(app): State<App>, Json(body): Json<Value>) -> Reply {
    let Some(sats) = body["sats"].as_i64() else {
        return detail(StatusCode::BAD_REQUEST, "sats is required.");
    };
    let mut inner = app.inner.lock().unwrap();
    inner.balance_msat += sats * 1000;
    (StatusCode::OK, Json(json!({ "balance_sats": inner.balance_msat / 1000 })))
}

async fn sandbox_state(State(app): State<App>) -> Json<Value> {
    let inner = app.inner.lock().unwrap();
    Json(json!({
        "balance_sats": inner.balance_msat / 1000,
        "payments": inner.records.iter().rev().take(50).map(Record::json).collect::<Vec<_>>(),
    }))
}

async fn sandbox_page() -> Html<&'static str> {
    Html(PAGE)
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>LNbits sandbox</title>
<style>
:root{color-scheme:light dark;--bg:#f6f5f2;--card:#fff;--text:#1b1a17;--muted:#6b675f;--line:#e2dfd8;--accent:#f7931a}
@media(prefers-color-scheme:dark){:root{--bg:#151412;--card:#201f1c;--text:#f1efe9;--muted:#a39f95;--line:#36342f}}
body{margin:0;background:var(--bg);color:var(--text);font:16px/1.5 system-ui,sans-serif}
main{max-width:640px;margin:0 auto;padding:20px 16px}
h1{font-size:1.3rem;margin:0 0 4px}p{margin:0 0 16px;color:var(--muted)}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px;margin-bottom:10px;display:flex;gap:12px;justify-content:space-between;align-items:center}
.card div{min-width:0}.small{font-size:.8rem;color:var(--muted);overflow-wrap:anywhere}
button{font:inherit;font-weight:600;border:0;border-radius:8px;padding:10px 16px;background:var(--accent);color:#1b1a17;cursor:pointer;min-height:44px}
h2{font-size:1rem;margin:20px 0 8px}
</style></head><body><main>
<h1>LNbits sandbox</h1>
<p>A stand-in for a Lightning node. Press <b>Pay</b> to play the sender's wallet paying an invoice SatsBridge created. Balance: <b id="bal">…</b> sats.</p>
<h2>Waiting to be paid</h2><div id="pending"></div>
<h2>Recent</h2><div id="recent"></div>
</main><script>
const el=(t,c,x)=>{const e=document.createElement(t);if(c)e.className=c;if(x!==undefined)e.textContent=x;return e};
async function pay(h){await fetch('/sandbox/pay/'+h,{method:'POST'});load()}
function row(p,withBtn){const c=el('div','card'),d=el('div');
 d.append(el('b','',p.amount_sats.toLocaleString()+' sats · '+(p.direction==='in'?'invoice':'sent')+' · '+p.status),el('div','small',p.memo||p.payment_hash.slice(0,16)));
 c.append(d);if(withBtn){const b=el('button','','Pay');b.onclick=()=>pay(p.payment_hash);c.append(b)}return c}
async function load(){try{const s=await (await fetch('/sandbox/state')).json();
 document.getElementById('bal').textContent=s.balance_sats.toLocaleString();
 const pend=document.getElementById('pending'),rec=document.getElementById('recent');pend.replaceChildren();rec.replaceChildren();
 const wait=s.payments.filter(p=>p.direction==='in'&&p.status==='pending');
 if(!wait.length)pend.append(el('div','small','Nothing waiting. Create a payment in SatsBridge first.'));
 wait.forEach(p=>pend.append(row(p,true)));
 s.payments.filter(p=>!wait.includes(p)).slice(0,10).forEach(p=>rec.append(row(p,false)))}catch(e){}}
load();setInterval(load,1500)
</script></body></html>"#;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let bind: SocketAddr = env("LNBITS_SANDBOX_BIND")
        .unwrap_or_else(|| "127.0.0.1:5000".into())
        .parse()?;
    let invoice_key = env("LNBITS_INVOICE_KEY").unwrap_or_else(|| "sandbox-invoice-key".into());
    let admin_key = env("LNBITS_ADMIN_KEY").unwrap_or_else(|| "sandbox-admin-key".into());
    if invoice_key == admin_key {
        anyhow::bail!("LNBITS_INVOICE_KEY and LNBITS_ADMIN_KEY must differ");
    }
    let balance_sats: i64 = env("SANDBOX_LN_BALANCE_SATS").and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let app: App = Arc::new(Sandbox {
        invoice_key: invoice_key.clone(),
        admin_key: admin_key.clone(),
        http: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?,
        inner: Mutex::new(Inner {
            records: Vec::new(),
            balance_msat: balance_sats * 1000,
        }),
    });
    let router = Router::new()
        .route("/api/v1/wallet", get(wallet))
        .route("/api/v1/payments", post(create_payment))
        .route("/api/v1/payments/{hash}", get(payment_status))
        .route("/sandbox", get(sandbox_page))
        .route("/sandbox/state", get(sandbox_state))
        .route("/sandbox/fund", post(sandbox_fund))
        .route("/sandbox/pay/{hash}", post(sandbox_pay))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("LNbits sandbox stand-in listening on http://{bind}  (balance {balance_sats} sats)");
    eprintln!("  pay invoices at http://{bind}/sandbox");
    eprintln!("  LNBITS_URL=http://{bind}");
    eprintln!("  LNBITS_INVOICE_KEY={invoice_key}");
    eprintln!("  LNBITS_ADMIN_KEY={admin_key}");
    axum::serve(listener, router).await?;
    Ok(())
}
