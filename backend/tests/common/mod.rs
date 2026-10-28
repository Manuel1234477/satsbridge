//! Shared harness for the end-to-end tests: the real router over a fresh SQLite file, a
//! test-only Lightning node, and the mock payout rails. Settlement always goes through the
//! production paths (the LNbits webhook, which re-checks with the node, or the watcher).
#![allow(dead_code)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use bitcoin::{
    hashes::{Hash, sha256},
    secp256k1::{Secp256k1, SecretKey},
};
use http_body_util::BodyExt;
use satsbridge::{
    AppState, Config,
    adapters::{
        AdapterError,
        fx::StaticFxProvider,
        lightning::{Invoice, InvoiceDescription, InvoiceStatus, LightningAdapter, PaymentSent, lnbits::LnbitsConfig},
        payout::{
            PayoutRegistry, mobile_money_mock::MockMobileMoneyAdapter, ngn_mock::MockNgnAdapter,
            tando_mock::MockTandoAdapter,
        },
    },
    config::{FxConfig, LightningConfig, PayoutConfig, RateLimitConfig, RecoveryConfig, parse_prices},
    domain::Currency,
    router,
    security::{SIGNATURE_HEADER, sign},
    state::connect,
};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const PROVIDER_SECRET: &str = "test-provider-secret";
pub const ADMIN_KEY: &str = "test-admin-key";
pub const PUBLIC_URL: &str = "https://satsbridge.test";
/// Round test prices: 1 XOF = 2.5 NGN before fees.
pub const TEST_PRICES: &str = "XOF=60000000,XAF=60000000,GNF=860000000,KES=12900000,NGN=150000000";

pub fn test_config(quote_ttl_secs: i64) -> Config {
    let db_path = std::env::temp_dir().join(format!("satsbridge-test-{}.db", uuid::Uuid::new_v4()));
    Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: format!("sqlite://{}", db_path.display()),
        public_base_url: Some(PUBLIC_URL.into()),
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
        invoice_expiry_secs: 600,
        resolution_ttl_secs: 600,
        fee_bps: 100,
        flat_fee_sats: 0,
        fx: FxConfig::static_only(parse_prices(TEST_PRICES).unwrap()),
        payouts: PayoutConfig::mock(50_000_000),
        // Unused: tests inject `TestNode` directly.
        lightning: LightningConfig {
            lnbits: LnbitsConfig {
                base_url: "http://127.0.0.1:9".into(),
                invoice_key: "unused".into(),
                admin_key: None,
                webhook_url: None,
            },
            poll_secs: 0,
        },
        dev_routes: true,
    }
}

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    pub ngn: Arc<MockNgnAdapter>,
    pub node: Arc<TestNode>,
    /// Bearer token of the default test user.
    pub token: String,
}

pub async fn app_with(quote_ttl_secs: i64) -> TestApp {
    build(quote_ttl_secs, |_, _, _| {}).await
}

/// `customize` may change config, add or replace payout rails, and set up the node.
pub async fn build(
    quote_ttl_secs: i64,
    customize: impl FnOnce(&mut Config, &mut PayoutRegistry, &Arc<TestNode>),
) -> TestApp {
    let mut config = test_config(quote_ttl_secs);
    let ngn = Arc::new(MockNgnAdapter::new(config.payouts.ngn_max_payout_minor));
    let mut payouts = PayoutRegistry::default();
    payouts.register(Arc::new(MockTandoAdapter));
    payouts.register(ngn.clone());
    payouts.register(Arc::new(MockMobileMoneyAdapter::new(Currency::Xof)));
    payouts.register(Arc::new(MockMobileMoneyAdapter::new(Currency::Xaf)));
    let node = Arc::new(TestNode::default());
    customize(&mut config, &mut payouts, &node);
    let pool = connect(&config.database_url).await.unwrap();
    let fx = Arc::new(StaticFxProvider::new(config.fx.static_prices.clone()));
    let state = AppState::assemble(config, pool, fx, node.clone(), payouts);
    let mut app = TestApp {
        router: router(state.clone()),
        state,
        ngn,
        node,
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

    /// Quote XOF -> NGN, resolve the account, create the payment and fetch its invoice.
    pub async fn ngn_payment_awaiting_invoice(&self) -> (String, Value) {
        let quote = self
            .quote(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 100_000 }))
            .await;
        assert_eq!(quote["destination_amount"], 24_749_850);

        let (status, resolved) = self
            .call(
                Method::POST,
                "/api/v1/recipients/resolve",
                Some(json!({ "currency": "NGN", "bank_code": "058", "account_number": "0123456789" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{resolved}");
        assert_eq!(resolved["account_name"], "DEMO RECIPIENT 6789");

        let id = self
            .create_payment(
                &quote,
                json!({
                    "rail": "NG_BANK_TRANSFER",
                    "bank_code": "058",
                    "account_number": "0123456789",
                    "resolution_id": resolved["resolution_id"],
                }),
            )
            .await;
        let invoice = self.invoice_for(&id).await;
        (id, invoice)
    }

    pub async fn invoice_for(&self, payment_id: &str) -> Value {
        let (status, invoice) = self
            .call(Method::POST, &format!("/api/v1/payments/{payment_id}/pay"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{invoice}");
        invoice
    }

    /// The sender pays: the node now reports the invoice paid, and LNbits fires its webhook.
    pub async fn pay_and_notify(&self, payment_hash: &Value) -> Value {
        let hash = payment_hash.as_str().unwrap();
        self.node.mark_paid(hash);
        self.lnbits_webhook(hash).await
    }

    /// LNbits posts the payment as a JSON-encoded string.
    pub async fn lnbits_webhook(&self, payment_hash: &str) -> Value {
        let body = json!(json!({ "payment_hash": payment_hash, "amount": 1 }).to_string());
        let (status, body) = self
            .raw(Method::POST, "/api/v1/webhooks/lnbits", None, Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    pub async fn status_of(&self, id: &str) -> Value {
        self.call(Method::GET, &format!("/api/v1/payments/{id}"), None).await.1
    }

    /// Mock provider callback via the dev route.
    pub async fn mock_payout(&self, id: &str, status: &str) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            &format!("/api/v1/dev/payments/{id}/payout"),
            Some(json!({ "status": status })),
        )
        .await
    }
}

/// A signed regtest BOLT11 invoice, e.g. for refunds.
pub fn bolt11(amount_msat: u64, description_hash_of: Option<&str>) -> (String, String) {
    let key = SecretKey::from_slice(&[5; 32]).unwrap();
    let hash = sha256::Hash::hash(uuid::Uuid::new_v4().as_bytes());
    let builder = lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Regtest);
    let pr = match description_hash_of {
        Some(metadata) => builder
            .description_hash(sha256::Hash::hash(metadata.as_bytes()))
            .payment_hash(hash),
        None => builder.description("refund".into()).payment_hash(hash),
    }
    .payment_secret(lightning_invoice::PaymentSecret([6; 32]))
    .current_timestamp()
    .min_final_cltv_expiry_delta(144)
    .amount_milli_satoshis(amount_msat)
    .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
    .unwrap()
    .to_string();
    (pr, hash.to_string())
}

/// Test-only Lightning node. Issues real (regtest) BOLT11 invoices, committing to the LNURL
/// metadata like LNbits does; an invoice is paid once the test calls `mark_paid`.
#[derive(Default)]
pub struct TestNode {
    issued: Mutex<std::collections::HashMap<String, (i64, bool)>>,
    sent: AtomicUsize,
    /// Outgoing payments made, by payment hash.
    pub paid_out: Mutex<Vec<String>>,
    /// When set, outgoing payments fail with this error.
    pub fail_sends: Mutex<Option<AdapterError>>,
}

impl TestNode {
    pub fn mark_paid(&self, payment_hash: &str) {
        self.issued
            .lock()
            .unwrap()
            .get_mut(payment_hash)
            .expect("unknown invoice")
            .1 = true;
    }

    pub fn sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LightningAdapter for TestNode {
    fn name(&self) -> &'static str {
        "test-node"
    }

    async fn create_invoice(
        &self,
        amount_sats: i64,
        description: &InvoiceDescription,
        expiry_secs: i64,
    ) -> Result<Invoice, AdapterError> {
        let (payment_request, payment_hash) = bolt11(amount_sats as u64 * 1000, description.lnurl_metadata.as_deref());
        self.issued
            .lock()
            .unwrap()
            .insert(payment_hash.clone(), (amount_sats, false));
        Ok(Invoice {
            payment_request,
            payment_hash,
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(expiry_secs),
        })
    }

    async fn pay_invoice(&self, pr: &str, _max_fee_sats: i64) -> Result<PaymentSent, AdapterError> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        if let Some(err) = self.fail_sends.lock().unwrap().take() {
            return Err(err);
        }
        let hash = satsbridge::adapters::lightning::decode_invoice(pr)?.payment_hash;
        self.paid_out.lock().unwrap().push(hash.clone());
        Ok(PaymentSent {
            payment_hash: hash,
            preimage: String::new(),
            fee_sats: 0,
        })
    }

    async fn invoice_status(&self, payment_hash: &str) -> Result<InvoiceStatus, AdapterError> {
        if self.paid_out.lock().unwrap().iter().any(|h| h == payment_hash) {
            return Ok(InvoiceStatus::Paid { amount_sats: 0 });
        }
        Ok(match self.issued.lock().unwrap().get(payment_hash) {
            Some(&(amount_sats, true)) => InvoiceStatus::Paid { amount_sats },
            Some(_) => InvoiceStatus::Pending,
            None => InvoiceStatus::Failed,
        })
    }
}
