//! Shared harness for the end-to-end tests: the real router over a fresh SQLite file, the
//! simulated Stellar ledger (priced from fixed test rates, no spread) and test-double collection
//! and payout rails (see `mocks`). Provider events (deposits, payout and refund results) go
//! through the same service handlers the signed webhooks call.
#![allow(dead_code)]

pub mod mocks;

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use stellarbridge::{
    AppState, Config,
    adapters::{
        collection::CollectionRegistry,
        fx::StaticFxProvider,
        payout::PayoutRegistry,
        stellar::simulated::SimulatedLedger,
    },
    config::{CollectionConfig, FxConfig, RateLimitConfig, RecoveryConfig, StellarConfig, parse_rates},
    db,
    domain::Currency,
    router,
    security::{SIGNATURE_HEADER, sign},
    services::{
        new_id,
        payments::{self, DepositEvent, PayoutEvent, PayoutEventStatus},
    },
    state::connect,
};
use mocks::{
    collection::{MockCollection, demo_sender},
    mobile_money::MockMobileMoneyAdapter,
    ngn::MockNgnAdapter,
    tando::MockTandoAdapter,
};
use tower::ServiceExt;

pub const PROVIDER_SECRET: &str = "test-provider-secret";
pub const ADMIN_KEY: &str = "test-admin-key";
/// Round test rates (units per USD): ₦1,500 = KSh 130 = $1.
pub const TEST_RATES: &str = "NGN=1500,KES=130,GHS=15,XOF=600,XAF=600,GNF=8600";

/// ₦150,000, the standard test payment.
pub const SEND_KOBO: i64 = 15_000_000;
/// ₦148,500 after the 1% fee = $99 = KSh 12,870, minus the 0.5% slippage buffer.
pub const PAYOUT_KES_CENTS: i64 = 1_280_565;
/// What the DEX actually delivers for ₦148,500, in stroops.
pub const DELIVERED_KES_STROOPS: i64 = 128_700_000_000;

pub fn test_config(quote_ttl_secs: i64) -> Config {
    let db_path = std::env::temp_dir().join(format!("stellarbridge-test-{}.db", uuid::Uuid::new_v4()));
    let mut stellar = StellarConfig::simulated();
    stellar.simulated_spread_bps = 0;
    Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: format!("sqlite://{}", db_path.display()),
        public_base_url: None,
        admin_api_key: Some(ADMIN_KEY.into()),
        cors_allowed_origins: vec![],
        trust_proxy_headers: false,
        rate_limit: RateLimitConfig {
            per_minute: 10_000,
            signups_per_hour: 10_000,
        },
        recovery: RecoveryConfig::defaults(),
        provider_webhook_secret: PROVIDER_SECRET.into(),
        quote_ttl_secs,
        resolution_ttl_secs: 600,
        fee_bps: 100,
        flat_fee_minor: 0,
        slippage_bps: 50,
        fx: FxConfig::static_only(parse_rates(TEST_RATES).unwrap()),
        stellar,
        collections: CollectionConfig {
            deposit_expiry_secs: 1_800,
        },
    }
}

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    /// The NGN bank rail; in these flows it only carries refunds.
    pub ngn: Arc<MockNgnAdapter>,
    pub ledger: Arc<SimulatedLedger>,
    /// Bearer token of the default test user.
    pub token: String,
}

pub async fn app_with(quote_ttl_secs: i64) -> TestApp {
    build(quote_ttl_secs, |_, _| {}).await
}

/// `customize` may change config and add or replace payout rails.
pub async fn build(quote_ttl_secs: i64, customize: impl FnOnce(&mut Config, &mut PayoutRegistry)) -> TestApp {
    let mut config = test_config(quote_ttl_secs);
    let ngn = Arc::new(MockNgnAdapter::new(50_000_000));
    let mut payouts = PayoutRegistry::default();
    payouts.register(Arc::new(MockTandoAdapter));
    payouts.register(ngn.clone());
    for currency in [Currency::Xof, Currency::Xaf, Currency::Gnf, Currency::Ghs] {
        payouts.register(Arc::new(MockMobileMoneyAdapter::new(currency)));
    }
    customize(&mut config, &mut payouts);
    let pool = connect(&config.database_url).await.unwrap();
    let fx = Arc::new(StaticFxProvider::new(config.fx.static_rates.clone()));
    let ledger = Arc::new(SimulatedLedger::new(
        fx.clone(),
        config.stellar.simulated_spread_bps,
        config.stellar.tx_timeout_secs,
    ));
    let mut collections = CollectionRegistry::default();
    for rail in MockCollection::all(500_000_000) {
        collections.register(Arc::new(rail));
    }
    let state = AppState::assemble(config, pool, fx, ledger.clone(), collections, payouts);
    let mut app = TestApp {
        router: router(state.clone()),
        state,
        ngn,
        ledger,
        token: String::new(),
    };
    app.token = app.new_user().await;
    app
}

impl TestApp {
    pub async fn new_user(&self) -> String {
        let (status, body) = self.raw(Method::POST, "/api/v1/users", None, Some(json!({}))).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["token"].as_str().unwrap().to_string()
    }

    /// Request as the default user.
    pub async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.raw(method, uri, Some(&self.token), body).await
    }

    pub async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.raw(method, uri, Some(ADMIN_KEY), body).await
    }

    pub async fn raw(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        self.send(req.body(body).unwrap()).await
    }

    pub async fn webhook(&self, path: &str, secret: &str, body: Value) -> (StatusCode, Value) {
        let raw = body.to_string();
        let req = Request::post(path)
            .header("content-type", "application/json")
            .header(SIGNATURE_HEADER, sign(secret, raw.as_bytes()))
            .body(Body::from(raw))
            .unwrap();
        self.send(req).await
    }

    pub async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    pub async fn quote(&self, body: Value) -> Value {
        let (status, quote) = self.call(Method::POST, "/api/v1/quotes", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{quote}");
        quote
    }

    pub async fn create_payment(&self, quote: &Value, recipient: Value) -> String {
        let (status, payment) = self
            .call(
                Method::POST,
                "/api/v1/payments",
                Some(json!({ "quote_id": quote["id"], "recipient": recipient })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{payment}");
        payment["id"].as_str().unwrap().to_string()
    }

    /// Quote ₦150,000 -> KES, create the payment to an M-Pesa number and get deposit instructions.
    pub async fn kes_payment_awaiting_deposit(&self) -> (String, Value) {
        let quote = self
            .quote(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": SEND_KOBO }))
            .await;
        assert_eq!(quote["destination_amount"], PAYOUT_KES_CENTS, "{quote}");
        let id = self
            .create_payment(
                &quote,
                json!({ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }),
            )
            .await;
        let deposit = self.pay(&id).await;
        (id, deposit)
    }

    pub async fn pay(&self, payment_id: &str) -> Value {
        let (status, deposit) = self
            .call(Method::POST, &format!("/api/v1/payments/{payment_id}/pay"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{deposit}");
        deposit
    }

    /// The sender's payment arriving at the collection rail, as the provider's webhook reports it.
    /// `body` may set `amount_minor` and `sender`, or `anonymous: true` for an unknown source account.
    pub async fn deposit(&self, payment_id: &str, body: Option<Value>) -> Value {
        let deposit = db::get_deposit(&self.state.pool, payment_id)
            .await
            .unwrap()
            .expect("call pay first");
        let body = body.unwrap_or(Value::Null);
        let sender = if body["anonymous"].as_bool().unwrap_or(false) {
            None
        } else if body["sender"].is_null() {
            Some(demo_sender(deposit.currency))
        } else {
            Some(serde_json::from_value(body["sender"].clone()).unwrap())
        };
        let event = DepositEvent {
            event_id: new_id("testdep"),
            reference: deposit.reference,
            amount_minor: body["amount_minor"].as_i64().unwrap_or(deposit.expected_amount),
            currency: deposit.currency,
            sender,
        };
        let outcome = payments::handle_deposit(&self.state, event).await.unwrap();
        json!({ "outcome": outcome })
    }

    pub async fn status_of(&self, id: &str) -> Value {
        self.call(Method::GET, &format!("/api/v1/payments/{id}"), None).await.1
    }

    /// The payout provider's result callback.
    pub async fn mock_payout(&self, id: &str, status: &str) -> (StatusCode, Value) {
        let payout = db::get_payout(&self.state.pool, id).await.unwrap();
        self.provider_result(payout.and_then(|p| p.provider_reference), status).await
    }

    /// The refund provider's result callback.
    pub async fn mock_refund(&self, id: &str, status: &str) -> (StatusCode, Value) {
        let refund = db::get_refund(&self.state.pool, id).await.unwrap();
        self.provider_result(refund.and_then(|r| r.provider_reference), status).await
    }

    async fn provider_result(&self, reference: Option<String>, status: &str) -> (StatusCode, Value) {
        let Some(provider_reference) = reference else {
            return (StatusCode::CONFLICT, Value::Null);
        };
        let event = PayoutEvent {
            event_id: new_id("testpo"),
            provider_reference,
            status: serde_json::from_value::<PayoutEventStatus>(json!(status)).unwrap(),
            reason: None,
        };
        match payments::handle_payout_event(&self.state, event).await {
            Ok(outcome) => (StatusCode::OK, json!({ "outcome": outcome })),
            Err(e) => (axum::response::IntoResponse::into_response(e).status(), Value::Null),
        }
    }
}

/// Kinds and statuses of a payment's Stellar transactions, e.g. `[("settle", "succeeded")]`.
pub fn stellar_steps(details: &Value) -> Vec<(String, String)> {
    details["stellar_transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["kind"].as_str().unwrap().to_string(),
                t["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

pub fn steps(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

pub fn to_statuses(details: &Value) -> Vec<String> {
    details["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["to_status"].as_str().map(String::from))
        .collect()
}
