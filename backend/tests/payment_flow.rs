//! End-to-end flow against the real router with mock adapters (PRD §17-18, NGN addendum A.9).

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use satsbridge::{
    AppState, Config,
    adapters::{
        AdapterError,
        fx::StaticFxProvider,
        lightning::{Invoice, InvoiceStatus, LightningAdapter, MockLightningAdapter, PaymentSent},
        payout::{
            PayoutRegistry,
            bitnob::BitnobConfig,
            lightning_address::{LightningAddressAdapter, LightningAddressConfig},
            ngn_mock::MockNgnAdapter,
            tando_mock::MockTandoAdapter,
        },
    },
    config::{FxConfig, LightningConfig, PayoutConfig, parse_prices},
    domain::Currency,
    router,
    security::{SIGNATURE_HEADER, sign},
    state::connect,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const LN_SECRET: &str = "test-ln-secret";
const PROVIDER_SECRET: &str = "test-provider-secret";
const TEST_PRICES: &str = "XOF=60000000,XAF=60000000,GNF=860000000,KES=12900000,NGN=150000000";

struct TestApp {
    router: Router,
    state: AppState,
    ngn: Arc<MockNgnAdapter>,
}

async fn app_with(quote_ttl_secs: i64) -> TestApp {
    build(quote_ttl_secs, |_, _, _| {}).await
}

async fn build(
    quote_ttl_secs: i64,
    customize: impl FnOnce(&mut Config, &mut PayoutRegistry, &mut Arc<dyn LightningAdapter>),
) -> TestApp {
    let db_path = std::env::temp_dir().join(format!("satsbridge-test-{}.db", uuid::Uuid::new_v4()));
    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: format!("sqlite://{}", db_path.display()),
        api_key: None,
        lightning_webhook_secret: LN_SECRET.into(),
        provider_webhook_secret: PROVIDER_SECRET.into(),
        quote_ttl_secs,
        invoice_expiry_secs: 600,
        resolution_ttl_secs: 600,
        fee_bps: 100,
        flat_fee_sats: 0,
        // Round test prices: 1 XOF = 2.5 NGN before fees.
        fx: FxConfig::static_only(parse_prices(TEST_PRICES).unwrap()),
        payouts: PayoutConfig::mock(50_000_000),
        lightning: LightningConfig::mock(),
        dev_routes: true,
    };
    let mut config = config;
    let ngn = Arc::new(MockNgnAdapter::new(config.payouts.ngn_max_payout_minor));
    let mut payouts = PayoutRegistry::default();
    payouts.register(Arc::new(MockTandoAdapter));
    payouts.register(ngn.clone());
    let mut lightning: Arc<dyn LightningAdapter> = Arc::new(MockLightningAdapter);
    customize(&mut config, &mut payouts, &mut lightning);
    let state = AppState {
        pool: connect(&config.database_url).await.unwrap(),
        fx: Arc::new(StaticFxProvider::new(config.fx.static_prices.clone())),
        lightning,
        payouts: Arc::new(payouts),
        config: Arc::new(config),
    };
    TestApp {
        router: router(state.clone()),
        state,
        ngn,
    }
}

impl TestApp {
    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        self.send(req.body(body).unwrap()).await
    }

    async fn webhook(&self, path: &str, secret: &str, body: Value) -> (StatusCode, Value) {
        let raw = body.to_string();
        let req = Request::post(path)
            .header("content-type", "application/json")
            .header(SIGNATURE_HEADER, sign(secret, raw.as_bytes()))
            .body(Body::from(raw))
            .unwrap();
        self.send(req).await
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// Quote XOF -> NGN, resolve the account, create the payment and fetch its invoice.
    async fn ngn_payment_awaiting_invoice(&self) -> (String, Value) {
        let (status, quote) = self
            .call(
                Method::POST,
                "/api/v1/quotes",
                Some(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 100_000 })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{quote}");
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

        let (status, payment) = self
            .call(
                Method::POST,
                "/api/v1/payments",
                Some(json!({
                    "quote_id": quote["id"],
                    "recipient": {
                        "rail": "NG_BANK_TRANSFER",
                        "bank_code": "058",
                        "account_number": "0123456789",
                        "resolution_id": resolved["resolution_id"],
                    }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{payment}");
        assert_eq!(payment["status"], "QUOTED");
        assert_eq!(payment["recipient_masked"], "058:******6789");
        assert!(
            payment.get("recipient").is_none(),
            "full account details must not be returned"
        );
        let id = payment["id"].as_str().unwrap().to_string();

        let (status, invoice) = self
            .call(Method::POST, &format!("/api/v1/payments/{id}/pay"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{invoice}");
        (id, invoice)
    }

    async fn status_of(&self, id: &str) -> Value {
        self.call(Method::GET, &format!("/api/v1/payments/{id}"), None).await.1
    }
}

#[tokio::test]
async fn xof_to_ngn_completes_with_exactly_one_payout() {
    let app = app_with(120).await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    assert!(invoice["payment_request"].as_str().unwrap().starts_with("lnbcrt"));

    // Requesting the invoice again returns the same one.
    let (_, again) = app
        .call(Method::POST, &format!("/api/v1/payments/{id}/pay"), None)
        .await;
    assert_eq!(again["payment_hash"], invoice["payment_hash"]);

    let settle =
        json!({ "event_id": "ln-1", "payment_hash": invoice["payment_hash"], "amount_sats": invoice["amount_sats"] });
    let (status, body) = app
        .webhook("/api/v1/webhooks/lightning", LN_SECRET, settle.clone())
        .await;
    assert_eq!((status, body["outcome"].as_str()), (StatusCode::OK, Some("processed")));

    // Replayed event, and a different event for the same invoice: neither pays out again.
    let (_, body) = app.webhook("/api/v1/webhooks/lightning", LN_SECRET, settle).await;
    assert_eq!(body["outcome"], "duplicate");
    let other =
        json!({ "event_id": "ln-2", "payment_hash": invoice["payment_hash"], "amount_sats": invoice["amount_sats"] });
    let (_, body) = app.webhook("/api/v1/webhooks/lightning", LN_SECRET, other).await;
    assert_eq!(body["outcome"], "ignored");
    assert_eq!(app.ngn.submissions(), 1);

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING");
    assert_eq!(details["payout"]["destination_amount"], 24_749_850);
    let reference = details["payout"]["provider_reference"].as_str().unwrap().to_string();
    assert_eq!(reference, format!("mockngn_{id}"));

    let success = json!({ "event_id": "po-1", "provider_reference": reference, "status": "success" });
    let (_, body) = app.webhook("/api/v1/webhooks/provider", PROVIDER_SECRET, success).await;
    assert_eq!(body["outcome"], "processed");
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "COMPLETED");

    let statuses: Vec<&str> = details["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["to_status"].as_str())
        .collect();
    assert_eq!(
        statuses,
        [
            "CREATED",
            "QUOTED",
            "AWAITING_PAYMENT",
            "SETTLED",
            "PAYOUT_PENDING",
            "COMPLETED"
        ]
    );

    // NGN reversal after success moves the payment to refund.
    let reversed = json!({ "event_id": "po-2", "provider_reference": reference, "status": "reversed" });
    app.webhook("/api/v1/webhooks/provider", PROVIDER_SECRET, reversed)
        .await;
    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "REFUND_PENDING");
    assert_eq!(details["payout"]["status"], "reversed");
}

#[tokio::test]
async fn dev_routes_drive_a_kes_payout() {
    let app = app_with(120).await;
    let (_, quote) = app
        .call(
            Method::POST,
            "/api/v1/quotes",
            Some(json!({ "source_currency": "XOF", "destination_currency": "KES", "amount": 50_000 })),
        )
        .await;
    let (status, payment) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": { "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{payment}");
    let id = payment["id"].as_str().unwrap();
    app.call(Method::POST, &format!("/api/v1/payments/{id}/pay"), None)
        .await;
    app.call(Method::POST, &format!("/api/v1/dev/payments/{id}/settle"), None)
        .await;
    app.call(
        Method::POST,
        &format!("/api/v1/dev/payments/{id}/payout"),
        Some(json!({ "status": "success" })),
    )
    .await;
    assert_eq!(app.status_of(id).await["status"], "COMPLETED");
}

#[tokio::test]
async fn rejects_bad_signatures_and_bad_recipients() {
    let app = app_with(120).await;
    let req = Request::post("/api/v1/webhooks/lightning")
        .header(SIGNATURE_HEADER, "deadbeef")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(app.send(req).await.0, StatusCode::UNAUTHORIZED);

    let (status, body) = app
        .call(
            Method::POST,
            "/api/v1/recipients/resolve",
            Some(json!({ "currency": "NGN", "bank_code": "058", "account_number": "12345" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_recipient");

    // NGN payment without a prior account resolution.
    let (_, quote) = app
        .call(
            Method::POST,
            "/api/v1/quotes",
            Some(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 100_000 })),
        )
        .await;
    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": { "rail": "NG_BANK_TRANSFER", "bank_code": "058", "account_number": "0123456789" } })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn expired_quotes_and_limits_are_enforced() {
    let app = app_with(0).await;
    let (_, quote) = app
        .call(
            Method::POST,
            "/api/v1/quotes",
            Some(json!({ "source_currency": "XOF", "destination_currency": "KES", "amount": 50_000 })),
        )
        .await;
    let (status, body) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": { "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" } })),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::GONE, Some("quote_expired"))
    );

    // ₦600,000 is above the ₦500,000 demo limit.
    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/quotes",
            Some(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 60_000_000, "amount_side": "destination" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, currencies) = app.call(Method::GET, "/api/v1/currencies", None).await;
    let ngn = currencies
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "NGN")
        .unwrap();
    assert_eq!(ngn["can_send"], false);
    assert_eq!(ngn["payout_rails"][0]["rail"], "NG_BANK_TRANSFER");
}

mod lnurl {
    use axum::{Json, Router, extract::Query, http::HeaderMap, routing::get};
    use bitcoin::{
        hashes::{Hash, sha256},
        secp256k1::{Secp256k1, SecretKey},
    };
    use lightning_invoice::{Currency as LnCurrency, InvoiceBuilder, PaymentSecret};
    use serde_json::{Value, json};

    const METADATA: &str = r#"[["text/plain","Pay ada"],["text/identifier","ada@tapnob.io"]]"#;

    fn invoice(amount_msat: u64) -> String {
        let key = SecretKey::from_slice(&[9; 32]).unwrap();
        InvoiceBuilder::new(LnCurrency::Regtest)
            .description_hash(sha256::Hash::hash(METADATA.as_bytes()))
            .payment_hash(sha256::Hash::hash(b"tapnob-preimage"))
            .payment_secret(PaymentSecret([2; 32]))
            .current_timestamp()
            .min_final_cltv_expiry_delta(144)
            .amount_milli_satoshis(amount_msat)
            .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
            .unwrap()
            .to_string()
    }

    /// Minimal LNURL-pay server for `ada@<addr>`; returns its `host:port`.
    pub async fn serve() -> String {
        let app = Router::new()
            .route(
                "/.well-known/lnurlp/ada",
                get(|headers: HeaderMap| async move {
                    let host = headers["host"].to_str().unwrap().to_string();
                    Json(json!({
                        "tag": "payRequest",
                        "callback": format!("http://{host}/cb"),
                        "minSendable": 1_000, "maxSendable": 1_000_000_000_000u64,
                        "metadata": METADATA,
                    }))
                }),
            )
            .route(
                "/cb",
                get(
                    |Query(q): Query<std::collections::HashMap<String, String>>| async move {
                        Json::<Value>(json!({ "pr": invoice(q["amount"].parse().unwrap()), "routes": [] }))
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr.to_string()
    }
}

#[tokio::test]
async fn tapnob_lightning_address_payout_completes_on_payment() {
    let domain = lnurl::serve().await;
    let allowed = domain.clone();
    let app = build(120, move |_, payouts, _| {
        payouts.register(Arc::new(LightningAddressAdapter::new(
            reqwest::Client::new(),
            Arc::new(MockLightningAdapter),
            LightningAddressConfig {
                provider: "tapnob",
                display_name: "Tapnob",
                currency: Currency::Ngn,
                allowed_domains: vec![allowed],
                allow_http: true,
            },
        )));
    })
    .await;

    let (status, quote) = app
        .call(
            Method::POST,
            "/api/v1/quotes",
            Some(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 100_000, "rail": "LIGHTNING_ADDRESS" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{quote}");
    assert_eq!(quote["destination_rail"], "LIGHTNING_ADDRESS");
    assert_eq!(quote["payout_amount_guaranteed"], false);

    // A bank recipient can't use a Lightning Address quote.
    let (status, _) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": { "rail": "NG_BANK_TRANSFER", "bank_code": "058", "account_number": "0123456789", "resolution_id": "x" } })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, payment) = app
        .call(
            Method::POST,
            "/api/v1/payments",
            Some(json!({ "quote_id": quote["id"], "recipient": { "rail": "LIGHTNING_ADDRESS", "address": format!("ada@{domain}") } })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{payment}");
    let id = payment["id"].as_str().unwrap();
    app.call(Method::POST, &format!("/api/v1/payments/{id}/pay"), None)
        .await;
    app.call(Method::POST, &format!("/api/v1/dev/payments/{id}/settle"), None)
        .await;

    let details = app.status_of(id).await;
    assert_eq!(details["status"], "COMPLETED", "{details}");
    assert_eq!(details["payout"]["provider"], "tapnob");
    assert_eq!(details["payout"]["status"], "completed");
    assert_eq!(app.ngn.submissions(), 0);
}

#[tokio::test]
async fn bitnob_webhooks_are_verified_and_mapped() {
    use hmac::{Hmac, Mac};
    const BITNOB_SECRET: &str = "bitnob-webhook-secret";
    let app = build(120, |config, _, _| {
        config.payouts.bitnob = Some(BitnobConfig {
            base_url: "http://127.0.0.1:9".into(),
            client_id: "c".into(),
            client_secret: "s".into(),
            webhook_secret: BITNOB_SECRET.into(),
            callback_url: "https://example.test/api/v1/webhooks/bitnob".into(),
            from_asset: "BTC".into(),
            min_payout_minor: 50_000,
            max_payout_minor: 50_000_000,
        });
    })
    .await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    let settle =
        json!({ "event_id": "ln-1", "payment_hash": invoice["payment_hash"], "amount_sats": invoice["amount_sats"] });
    app.webhook("/api/v1/webhooks/lightning", LN_SECRET, settle).await;
    let reference = app.status_of(&id).await["payout"]["provider_reference"]
        .as_str()
        .unwrap()
        .to_string();

    let send = |event: &str, event_id: &str, secret: &str| {
        let body = json!({ "event": event, "event_id": event_id, "data": { "id": reference, "status": "success" } })
            .to_string();
        let mut mac = Hmac::<sha2::Sha512>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body.as_bytes());
        Request::post("/api/v1/webhooks/bitnob")
            .header("content-type", "application/json")
            .header("x-bitnob-signature", hex::encode(mac.finalize().into_bytes()))
            .body(Body::from(body))
            .unwrap()
    };

    assert_eq!(
        app.send(send("payouts.withdrawal.success", "e0", "wrong")).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.send(send("payouts.processing", "e1", BITNOB_SECRET)).await.1["outcome"],
        "ignored"
    );
    assert_eq!(
        app.send(send("payouts.withdrawal.success", "e2", BITNOB_SECRET))
            .await
            .1["outcome"],
        "processed"
    );
    assert_eq!(
        app.send(send("payouts.withdrawal.success", "e2", BITNOB_SECRET))
            .await
            .1["outcome"],
        "duplicate"
    );
    assert_eq!(app.status_of(&id).await["status"], "COMPLETED");
}

/// Node that reports every invoice it issued as paid in full.
#[derive(Default)]
struct PaidNode {
    issued: std::sync::Mutex<std::collections::HashMap<String, i64>>,
}

#[async_trait::async_trait]
impl LightningAdapter for PaidNode {
    fn name(&self) -> &'static str {
        "paid-node"
    }

    async fn create_invoice(&self, amount_sats: i64, memo: &str, expiry_secs: i64) -> Result<Invoice, AdapterError> {
        let invoice = MockLightningAdapter
            .create_invoice(amount_sats, memo, expiry_secs)
            .await?;
        self.issued
            .lock()
            .unwrap()
            .insert(invoice.payment_hash.clone(), amount_sats);
        Ok(invoice)
    }

    async fn pay_invoice(&self, pr: &str, max_fee_sats: i64) -> Result<PaymentSent, AdapterError> {
        MockLightningAdapter.pay_invoice(pr, max_fee_sats).await
    }

    async fn invoice_status(&self, payment_hash: &str) -> Result<InvoiceStatus, AdapterError> {
        Ok(match self.issued.lock().unwrap().get(payment_hash) {
            Some(&amount_sats) => InvoiceStatus::Paid { amount_sats },
            None => InvoiceStatus::Failed,
        })
    }
}

#[tokio::test]
async fn watcher_settles_paid_invoices_once() {
    use satsbridge::services::settlement;
    let app = build(120, |_, _, lightning| *lightning = Arc::new(PaidNode::default())).await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;

    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 1);
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);
    assert_eq!(app.status_of(&id).await["status"], "PAYOUT_PENDING");

    // The LNbits webhook path re-checks with the node and deduplicates with the poller.
    let hook = json!(json!({ "payment_hash": invoice["payment_hash"] }).to_string());
    let (status, body) = app.call(Method::POST, "/api/v1/webhooks/lnbits", Some(hook)).await;
    assert_eq!((status, body["outcome"].as_str()), (StatusCode::OK, Some("duplicate")));
    assert_eq!(app.ngn.submissions(), 1);
}

#[tokio::test]
async fn watcher_expires_unpaid_invoices() {
    use satsbridge::services::settlement;
    let app = build(120, |config, _, _| config.invoice_expiry_secs = 0).await;
    let (id, _) = app.ngn_payment_awaiting_invoice().await;
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);
    assert_eq!(app.status_of(&id).await["status"], "EXPIRED");
}
