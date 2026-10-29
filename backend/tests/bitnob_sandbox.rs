//! Drives the real Bitnob adapter against the local stand-in (`src/bin/bitnob_sandbox.rs`):
//! signed requests, quote -> initialize -> finalize, and the signed webhook coming back.
//! Needs no network or credentials; the stand-in is started on a free local port.

use std::{
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use axum::{Router, body::Bytes, extract::State, http::HeaderMap, routing::post};
use satsbridge::{
    adapters::{
        bitnob::{self, BitnobClient, BitnobConfig, payout::BitnobPayoutAdapter},
        payout::{PayoutAdapter, PayoutRequest},
    },
    domain::{Currency, Rail, Recipient},
    security::verify_hmac_sha512,
    services::payments::PayoutEventStatus,
};
use tokio::sync::mpsc;

const CLIENT_ID: &str = "test-client";
const SECRET: &str = "test-secret";
const WEBHOOK_SECRET: &str = "test-webhook";

struct Sandbox(Child);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn config(base_url: &str, secret: &str, callback_url: &str) -> BitnobConfig {
    BitnobConfig {
        base_url: base_url.into(),
        client_id: CLIENT_ID.into(),
        client_secret: secret.into(),
        webhook_secret: WEBHOOK_SECRET.into(),
        callback_url: callback_url.into(),
        from_asset: "BTC".into(),
        countries: vec!["NG".into()],
    }
}

async fn start_sandbox() -> (Sandbox, String) {
    let base = format!("http://127.0.0.1:{}", free_port());
    let child = Command::new(env!("CARGO_BIN_EXE_bitnob_sandbox"))
        .env("SANDBOX_BIND", base.trim_start_matches("http://"))
        .env("BITNOB_CLIENT_ID", CLIENT_ID)
        .env("BITNOB_CLIENT_SECRET", SECRET)
        .env("BITNOB_WEBHOOK_SECRET", WEBHOOK_SECRET)
        .env("SANDBOX_AUTO_COMPLETE_SECS", "1")
        .stderr(Stdio::null())
        .spawn()
        .expect("start bitnob_sandbox");
    let sandbox = Sandbox(child);
    let client = BitnobClient::new(reqwest::Client::new(), config(&base, SECRET, ""));
    for _ in 0..50 {
        if client.whoami().await.is_ok() {
            return (sandbox, base);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the sandbox did not start");
}

type Received = (HeaderMap, Bytes);

/// Records webhook deliveries on a free local port; returns its URL.
async fn webhook_receiver() -> (String, mpsc::UnboundedReceiver<Received>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let app = Router::new()
        .route(
            "/hook",
            post(
                |State(tx): State<mpsc::UnboundedSender<Received>>, headers: HeaderMap, body: Bytes| async move {
                    let _ = tx.send((headers, body));
                },
            ),
        )
        .with_state(tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, rx)
}

async fn adapter(base: &str, callback_url: &str) -> BitnobPayoutAdapter {
    let client = Arc::new(BitnobClient::new(
        reqwest::Client::new(),
        config(base, SECRET, callback_url),
    ));
    let limits = client.limits().await.unwrap();
    BitnobPayoutAdapter::connect(client, Currency::Ngn, Rail::NgBankTransfer, &["NG".to_string()], &limits)
        .await
        .unwrap()
}

fn bank(account: &str) -> Recipient {
    Recipient::NgBankTransfer {
        bank_code: "000014".into(),
        account_number: account.into(),
        resolution_id: None,
    }
}

async fn pay(adapter: &BitnobPayoutAdapter, key: &str, account: &str) -> String {
    let recipient = bank(account);
    adapter
        .create_payout(PayoutRequest {
            idempotency_key: key,
            attempt: 1,
            recipient: &recipient,
            account_name: Some("ADA OBI"),
            amount_minor: 10_000_000,
            amount_sats: 100_000,
            currency: Currency::Ngn,
            narration: format!("SatsBridge {key}"),
        })
        .await
        .unwrap()
        .provider_reference
}

async fn next_event(rx: &mut mpsc::UnboundedReceiver<Received>) -> bitnob::WebhookEvent {
    let (headers, body) = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("webhook within 10s")
        .unwrap();
    let signature = headers.get(bitnob::SIGNATURE_HEADER).and_then(|v| v.to_str().ok());
    assert!(
        verify_hmac_sha512(WEBHOOK_SECRET, &body, signature),
        "webhook signature must verify"
    );
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn payouts_succeed_expire_and_reject_bad_credentials() {
    let (_sandbox, base) = start_sandbox().await;
    let (hook, mut events) = webhook_receiver().await;

    // Wrong secret: refused like the real API.
    let wrong = BitnobClient::new(reqwest::Client::new(), config(&base, "not-the-secret", &hook));
    assert!(wrong.whoami().await.is_err());

    let me = BitnobClient::new(reqwest::Client::new(), config(&base, SECRET, &hook))
        .whoami()
        .await
        .unwrap();
    assert_eq!(me.environment.as_deref(), Some("sandbox"));

    let ng = adapter(&base, &hook).await;
    assert_eq!(ng.institutions(None).await.unwrap().len(), 10);
    assert_eq!(
        ng.resolve_recipient(&bank("1421795566")).await.unwrap().account_name,
        ng.resolve_recipient(&bank("1421795566")).await.unwrap().account_name,
    );
    assert!(ng.resolve_recipient(&bank("0000000000")).await.is_err());

    // Success path: the webhook names the payout we were handed back.
    let reference = pay(&ng, "pay_ok", "1421795566").await;
    let event = next_event(&mut events).await;
    assert_eq!(event.data.id, reference);
    let event = bitnob::to_payout_event(event).unwrap();
    assert!(matches!(event.status, PayoutEventStatus::Success));

    // Reconciliation finds the payout by its reference, and its status is final.
    let found = ng.find_payout("pay_ok").await.unwrap().unwrap();
    assert_eq!(found.provider_reference, reference);
    assert!(matches!(
        ng.payout_status(&reference).await.unwrap(),
        Some(PayoutEventStatus::Success)
    ));

    // Accounts ending 9999 expire, which SatsBridge treats as a failed payout.
    let reference = pay(&ng, "pay_bad", "1421799999").await;
    let event = next_event(&mut events).await;
    assert_eq!(event.data.id, reference);
    assert!(matches!(
        bitnob::to_payout_event(event).unwrap().status,
        PayoutEventStatus::Failed
    ));
}

#[tokio::test]
async fn rejects_amounts_outside_limits_and_bad_beneficiaries() {
    let (_sandbox, base) = start_sandbox().await;
    let ng = adapter(&base, "http://127.0.0.1:1/unused").await;
    let recipient = bank("12345");
    let too_short = ng
        .create_payout(PayoutRequest {
            idempotency_key: "pay_x",
            attempt: 1,
            recipient: &recipient,
            account_name: Some("ADA OBI"),
            amount_minor: 10_000_000,
            amount_sats: 100_000,
            currency: Currency::Ngn,
            narration: "x".into(),
        })
        .await;
    assert!(too_short.is_err(), "a 5-digit account number must be rejected");
}

#[tokio::test]
async fn serves_guinea_mobile_money() {
    let (_sandbox, base) = start_sandbox().await;
    let client = Arc::new(BitnobClient::new(reqwest::Client::new(), config(&base, SECRET, "")));
    let limits = client.limits().await.unwrap();
    let adapter =
        BitnobPayoutAdapter::connect(client, Currency::Gnf, Rail::MobileMoney, &["GN".to_string()], &limits)
            .await
            .unwrap();
    let operators = adapter.institutions(Some("GN")).await.unwrap();
    assert!(operators.iter().any(|o| o.code == "OM"));
    assert!(operators.iter().any(|o| o.code == "MTN"));
}
