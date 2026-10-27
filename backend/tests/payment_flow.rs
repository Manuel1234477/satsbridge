//! End-to-end payment flows (PRD §17-18, NGN addendum A.9).

mod common;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::*;
use satsbridge::{
    adapters::{
        bitnob::BitnobConfig,
        payout::lightning_address::{LightningAddressAdapter, LightningAddressConfig},
    },
    domain::Currency,
    security::SIGNATURE_HEADER,
    services::settlement,
};
use serde_json::json;

#[tokio::test]
async fn xof_to_ngn_completes_with_exactly_one_payout() {
    let app = app_with(120).await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    assert!(invoice["payment_request"].as_str().unwrap().starts_with("lnbcrt"));

    // Requesting the invoice again returns the same one.
    assert_eq!(app.invoice_for(&id).await["payment_hash"], invoice["payment_hash"]);

    // A webhook for an invoice the node hasn't seen paid changes nothing.
    let hash = invoice["payment_hash"].as_str().unwrap();
    assert_eq!(app.lnbits_webhook(hash).await["outcome"], "ignored");
    assert_eq!(app.status_of(&id).await["status"], "AWAITING_PAYMENT");

    assert_eq!(
        app.pay_and_notify(&invoice["payment_hash"]).await["outcome"],
        "processed"
    );

    // Replayed webhook and a watcher pass: neither pays out again.
    assert_eq!(app.lnbits_webhook(hash).await["outcome"], "duplicate");
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);
    assert_eq!(app.ngn.submissions(), 1);

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "PAYOUT_PENDING");
    assert_eq!(details["payout"]["destination_amount"], 24_749_850);
    assert_eq!(details["payout"]["attempts"], 1);
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
    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "KES", "amount": 50_000 }))
        .await;
    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "KE_MOBILE_MONEY", "phone_number": "0712345678" }),
        )
        .await;
    let invoice = app.invoice_for(&id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;
    app.mock_payout(&id, "success").await;
    assert_eq!(app.status_of(&id).await["status"], "COMPLETED");
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
    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 100_000 }))
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
async fn ngn_can_be_the_source_currency() {
    let app = app_with(120).await;
    let quote = app
        .quote(json!({ "source_currency": "NGN", "destination_currency": "KES", "amount": 5_000_000 }))
        .await;
    assert_eq!(quote["source_currency"], "NGN");
    assert_eq!(quote["destination_currency"], "KES");
    assert_eq!(quote["source_amount"], 5_000_000);
}

#[tokio::test]
async fn expired_quotes_and_limits_are_enforced() {
    let app = app_with(0).await;
    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "KES", "amount": 50_000 }))
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

    let (_, currencies) = app.raw(Method::GET, "/api/v1/currencies", None, None).await;
    let ngn = currencies
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "NGN")
        .unwrap();
    assert!(ngn.get("can_send").is_none());
    assert_eq!(ngn["payout_rails"][0]["rail"], "NG_BANK_TRANSFER");
}

mod lnurl_server {
    use axum::{Json, Router, extract::Query, http::HeaderMap, routing::get};
    use serde_json::{Value, json};

    pub const METADATA: &str = r#"[["text/plain","Pay ada"],["text/identifier","ada@tapnob.io"]]"#;

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
                        let (pr, _) = super::bolt11(q["amount"].parse().unwrap(), Some(METADATA));
                        Json::<Value>(json!({ "pr": pr, "routes": [] }))
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr.to_string()
    }
}

pub fn tapnob_adapter(node: Arc<TestNode>, domain: String) -> LightningAddressAdapter {
    LightningAddressAdapter::new(
        reqwest::Client::new(),
        node,
        LightningAddressConfig {
            provider: "tapnob",
            display_name: "Tapnob",
            currency: Currency::Ngn,
            allowed_domains: vec![domain],
            allow_http: true,
        },
    )
}

#[tokio::test]
async fn tapnob_lightning_address_payout_completes_on_payment() {
    let domain = lnurl_server::serve().await;
    let allowed = domain.clone();
    let app = build(120, move |_, payouts, node| {
        payouts.register(Arc::new(tapnob_adapter(node.clone(), allowed)))
    })
    .await;

    let quote = app
        .quote(json!({ "source_currency": "XOF", "destination_currency": "NGN", "amount": 100_000, "rail": "LIGHTNING_ADDRESS" }))
        .await;
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

    let id = app
        .create_payment(
            &quote,
            json!({ "rail": "LIGHTNING_ADDRESS", "address": format!("ada@{domain}") }),
        )
        .await;
    let invoice = app.invoice_for(&id).await;
    app.pay_and_notify(&invoice["payment_hash"]).await;

    let details = app.status_of(&id).await;
    assert_eq!(details["status"], "COMPLETED", "{details}");
    assert_eq!(details["payout"]["provider"], "tapnob");
    assert_eq!(details["payout"]["status"], "completed");
    assert_eq!(app.ngn.submissions(), 0);
    assert_eq!(app.node.sent(), 1);

    // Real providers can't be "completed" through the dev simulator.
    assert_eq!(app.mock_payout(&id, "success").await.0, StatusCode::CONFLICT);
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
            countries: vec!["NG".into()],
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

#[tokio::test]
async fn watcher_settles_paid_invoices_once() {
    let app = app_with(120).await;
    let (id, invoice) = app.ngn_payment_awaiting_invoice().await;
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);

    // Paid without a webhook reaching us: the watcher picks it up.
    app.node.mark_paid(invoice["payment_hash"].as_str().unwrap());
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 1);
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);
    assert_eq!(app.status_of(&id).await["status"], "PAYOUT_PENDING");

    // The LNbits webhook path re-checks with the node and deduplicates with the poller.
    assert_eq!(
        app.lnbits_webhook(invoice["payment_hash"].as_str().unwrap()).await["outcome"],
        "duplicate"
    );
    assert_eq!(app.ngn.submissions(), 1);
}

#[tokio::test]
async fn watcher_expires_unpaid_invoices() {
    let app = build(120, |config, _, _| config.invoice_expiry_secs = 0).await;
    let (id, _) = app.ngn_payment_awaiting_invoice().await;
    assert_eq!(settlement::poll_once(&app.state).await.unwrap(), 0);
    assert_eq!(app.status_of(&id).await["status"], "EXPIRED");
}
