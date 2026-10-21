//! End-to-end flows through the real router (PRD §17-18, NGN addendum A.9). Lightning is a
//! test-only node (`TestNode`); settlement always goes through the production paths: the LNbits
//! webhook (which re-checks with the node) or the settlement watcher.

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
        lightning::{Invoice, InvoiceStatus, LightningAdapter, PaymentSent, lnbits::LnbitsConfig},
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

const PROVIDER_SECRET: &str = "test-provider-secret";
const TEST_PRICES: &str = "XOF=60000000,XAF=60000000,GNF=860000000,KES=12900000,NGN=150000000";

struct TestApp {
    router: Router,
    state: AppState,
    ngn: Arc<MockNgnAdapter>,
    node: Arc<TestNode>,
}

async fn app_with(quote_ttl_secs: i64) -> TestApp {
    build(quote_ttl_secs, |_, _, _| {}).await
}

async fn build(
    quote_ttl_secs: i64,
    customize: impl FnOnce(&mut Config, &mut PayoutRegistry, &Arc<TestNode>),
) -> TestApp {
    let db_path = std::env::temp_dir().join(format!("satsbridge-test-{}.db", uuid::Uuid::new_v4()));
    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: format!("sqlite://{}", db_path.display()),
        api_key: None,
        provider_webhook_secret: PROVIDER_SECRET.into(),
        quote_ttl_secs,
        invoice_expiry_secs: 600,
        resolution_ttl_secs: 600,
        fee_bps: 100,
        flat_fee_sats: 0,
        // Round test prices: 1 XOF = 2.5 NGN before fees.
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
    };
    let mut config = config;
    let ngn = Arc::new(MockNgnAdapter::new(config.payouts.ngn_max_payout_minor));
    let mut payouts = PayoutRegistry::default();
    payouts.register(Arc::new(MockTandoAdapter));
    payouts.register(ngn.clone());
    let node = Arc::new(TestNode::default());
    customize(&mut config, &mut payouts, &node);
    let state = AppState {
        pool: connect(&config.database_url).await.unwrap(),
        fx: Arc::new(StaticFxProvider::new(config.fx.static_prices.clone())),
        lightning: node.clone(),
        payouts: Arc::new(payouts),
        config: Arc::new(config),
    };
    TestApp {
        router: router(state.clone()),
        state,
        ngn,
        node,
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

    /// The sender pays: the node now reports the invoice paid, and LNbits fires its webhook.
    async fn pay_and_notify(&self, payment_hash: &Value) -> Value {
        let hash = payment_hash.as_str().unwrap();
        self.node.mark_paid(hash);
        self.lnbits_webhook(hash).await
    }

    /// LNbits posts the payment as a JSON-encoded string.
    async fn lnbits_webhook(&self, payment_hash: &str) -> Value {
        let body = json!(json!({ "payment_hash": payment_hash, "amount": 1 }).to_string());
        let (status, body) = self.call(Method::POST, "/api/v1/webhooks/lnbits", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn invoice_for(&self, payment_id: &str) -> Value {
        self.call(Method::POST, &format!("/api/v1/payments/{payment_id}/pay"), None)
            .await
            .1
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

    // A webhook for an invoice the node hasn't seen paid changes nothing.
    let hash = invoice["payment_hash"].as_str().unwrap();
    assert_eq!(app.lnbits_webhook(hash).await["outcome"], "ignored");
    assert_eq!(app.status_of(&id).await["status"], "AWAITING_PAYMENT");

    // Requesting the invoice again returns the same one.
    let (_, again) = app
        .call(Method::POST, &format!("/api/v1/payments/{id}/pay"), None)
        .await;
    assert_eq!(again["payment_hash"], invoice["payment_hash"]);

    assert_eq!(
        app.pay_and_notify(&invoice["payment_hash"]).await["outcome"],
        "processed"
    );

    // Replayed webhook and a watcher pass: neither pays out again.
    assert_eq!(app.lnbits_webhook(hash).await["outcome"], "duplicate");
    assert_eq!(
        satsbridge::services::settlement::poll_once(&app.state).await.unwrap(),
        0
    );
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
async fn kes_payout_via_mock_provider_callback() {
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
    let invoice = app.invoice_for(id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
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
    let req = Request::post("/api/v1/webhooks/provider")
        .header(SIGNATURE_HEADER, "deadbeef")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(app.send(req).await.0, StatusCode::UNAUTHORIZED);

    // There is no endpoint that settles a payment on the caller's word.
    let (status, _) = app
        .call(Method::POST, "/api/v1/webhooks/lightning", Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.call(Method::POST, "/api/v1/dev/payments/x/settle", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

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
    let app = build(120, move |_, payouts, node| {
        payouts.register(Arc::new(LightningAddressAdapter::new(
            reqwest::Client::new(),
            node.clone(),
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
    let invoice = app.invoice_for(id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;

    let details = app.status_of(id).await;
    assert_eq!(details["status"], "COMPLETED", "{details}");
    assert_eq!(details["payout"]["provider"], "tapnob");
    assert_eq!(details["payout"]["status"], "completed");
    assert_eq!(app.ngn.submissions(), 0);
    assert_eq!(app.node.sent(), 1);

    // Real providers can't be "completed" through the dev simulator.
    let (status, _) = app
        .call(
            Method::POST,
            &format!("/api/v1/dev/payments/{id}/payout"),
            Some(json!({ "status": "success" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
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
    app.pay_and_notify(&invoice["payment_hash"]).await;
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

/// Test-only Lightning node. Issues real (regtest) BOLT11 invoices; an invoice is paid once
/// the test calls `mark_paid`, as if the sender had paid it.
#[derive(Default)]
struct TestNode {
    issued: std::sync::Mutex<std::collections::HashMap<String, (i64, bool)>>,
    sent: std::sync::atomic::AtomicUsize,
}

impl TestNode {
    fn mark_paid(&self, payment_hash: &str) {
        self.issued
            .lock()
            .unwrap()
            .get_mut(payment_hash)
            .expect("unknown invoice")
            .1 = true;
    }

    fn sent(&self) -> usize {
        self.sent.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LightningAdapter for TestNode {
    fn name(&self) -> &'static str {
        "test-node"
    }

    async fn create_invoice(&self, amount_sats: i64, memo: &str, expiry_secs: i64) -> Result<Invoice, AdapterError> {
        use bitcoin::{
            hashes::{Hash, sha256},
            secp256k1::{Secp256k1, SecretKey},
        };
        let preimage = uuid::Uuid::new_v4();
        let hash = sha256::Hash::hash(preimage.as_bytes());
        let key = SecretKey::from_slice(&[5; 32]).unwrap();
        let payment_request = lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Regtest)
            .description(memo.into())
            .payment_hash(hash)
            .payment_secret(lightning_invoice::PaymentSecret([6; 32]))
            .current_timestamp()
            .min_final_cltv_expiry_delta(144)
            .amount_milli_satoshis(amount_sats as u64 * 1000)
            .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
            .unwrap()
            .to_string();
        self.issued
            .lock()
            .unwrap()
            .insert(hash.to_string(), (amount_sats, false));
        Ok(Invoice {
            payment_request,
            payment_hash: hash.to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(expiry_secs),
        })
    }

    async fn pay_invoice(&self, pr: &str, _max_fee_sats: i64) -> Result<PaymentSent, AdapterError> {
        self.sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let invoice: lightning_invoice::Bolt11Invoice = pr.parse().unwrap();
        Ok(PaymentSent {
            payment_hash: invoice.payment_hash().to_string(),
            preimage: String::new(),
            fee_sats: 0,
        })
    }

    async fn invoice_status(&self, payment_hash: &str) -> Result<InvoiceStatus, AdapterError> {
        Ok(match self.issued.lock().unwrap().get(payment_hash) {
            Some(&(amount_sats, true)) => InvoiceStatus::Paid { amount_sats },
            Some(_) => InvoiceStatus::Pending,
            None => InvoiceStatus::Failed,
        })
    }
}

#[tokio::test]
async fn watcher_settles_paid_invoices_once() {
    use satsbridge::services::settlement;
    let app = app_with(120).await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);

    // Paid without a webhook reaching us: the watcher picks it up.
    app.node.mark_paid(invoice["payment_hash"].as_str().unwrap());
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
