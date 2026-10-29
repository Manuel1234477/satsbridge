//! Drives the real LNbits adapter against the local stand-in (`src/bin/lnbits_sandbox.rs`):
//! key rules, signed invoices, settlement, the LNbits-style webhook, and outgoing payments with an
//! idempotent retry. Needs no network or credentials.

use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

use axum::{Router, body::Bytes, extract::State, routing::post};
use bitcoin::{
    hashes::{Hash, sha256},
    secp256k1::{Secp256k1, SecretKey},
};
use satsbridge::adapters::{
    AdapterError,
    lightning::{
        InvoiceDescription, InvoiceStatus, LightningAdapter, decode_invoice,
        lnbits::{LnbitsAdapter, LnbitsConfig, webhook_payment_hash},
    },
};
use tokio::sync::mpsc;

const INVOICE_KEY: &str = "test-invoice-key";
const ADMIN_KEY: &str = "test-admin-key";

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

fn adapter(base: &str, admin: bool, webhook: Option<String>) -> LnbitsAdapter {
    LnbitsAdapter::new(
        reqwest::Client::new(),
        LnbitsConfig {
            base_url: base.into(),
            invoice_key: INVOICE_KEY.into(),
            admin_key: admin.then(|| ADMIN_KEY.to_string()),
            webhook_url: webhook,
        },
    )
}

async fn start() -> (Sandbox, String) {
    let base = format!("http://127.0.0.1:{}", free_port());
    let child = Command::new(env!("CARGO_BIN_EXE_lnbits_sandbox"))
        .env("LNBITS_SANDBOX_BIND", base.trim_start_matches("http://"))
        .env("LNBITS_INVOICE_KEY", INVOICE_KEY)
        .env("LNBITS_ADMIN_KEY", ADMIN_KEY)
        .stderr(Stdio::null())
        .spawn()
        .expect("start lnbits_sandbox");
    let sandbox = Sandbox(child);
    for _ in 0..50 {
        if adapter(&base, false, None).verify_credentials().await.is_ok() {
            return (sandbox, base);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the sandbox did not start");
}

fn memo(text: &str) -> InvoiceDescription {
    InvoiceDescription {
        memo: text.into(),
        lnurl_metadata: None,
    }
}

/// A signed invoice from some other node, for outgoing payments.
fn foreign_invoice(amount_msat: u64) -> String {
    let key = SecretKey::from_slice(&[9; 32]).unwrap();
    let hash = sha256::Hash::hash(uuid::Uuid::new_v4().as_bytes());
    lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Regtest)
        .description("refund".into())
        .payment_hash(hash)
        .payment_secret(lightning_invoice::PaymentSecret([3; 32]))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .amount_milli_satoshis(amount_msat)
        .build_signed(|m| Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .unwrap()
        .to_string()
}

async fn sandbox_pay(base: &str, hash: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(format!("{base}/sandbox/pay/{hash}"))
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn invoices_settle_only_when_the_sandbox_reports_them_paid() {
    let (_sandbox, base) = start().await;

    // Webhook receiver, recording raw bodies.
    let (tx, mut hooks) = mpsc::unbounded_channel::<Bytes>();
    let app = Router::new()
        .route("/hook", post(|State(tx): State<mpsc::UnboundedSender<Bytes>>, body: Bytes| async move { let _ = tx.send(body); }))
        .with_state(tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hook_url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let ln = adapter(&base, true, Some(hook_url));
    let wallet = ln.verify_credentials().await.unwrap();
    assert_eq!(wallet.name, "SatsBridge sandbox wallet");

    // A real, signed invoice for the requested amount.
    let invoice = ln.create_invoice(21, &memo("coffee"), 600).await.unwrap();
    let decoded = decode_invoice(&invoice.payment_request).unwrap();
    assert_eq!(decoded.amount_msat, Some(21_000));
    assert_eq!(decoded.payment_hash, invoice.payment_hash);

    // Unpaid until the sender's wallet (the sandbox page) pays it.
    assert_eq!(ln.invoice_status(&invoice.payment_hash).await.unwrap(), InvoiceStatus::Pending);
    assert!(hooks.try_recv().is_err());
    assert_eq!(sandbox_pay(&base, &invoice.payment_hash).await, 200);
    assert_eq!(
        ln.invoice_status(&invoice.payment_hash).await.unwrap(),
        InvoiceStatus::Paid { amount_sats: 21 }
    );
    // Paying twice is refused.
    assert_eq!(sandbox_pay(&base, &invoice.payment_hash).await, 409);

    // The webhook is the double-encoded string LNbits sends, and names the invoice.
    let body = tokio::time::timeout(Duration::from_secs(5), hooks.recv()).await.unwrap().unwrap();
    assert_eq!(webhook_payment_hash(&body).as_deref(), Some(invoice.payment_hash.as_str()));

    // LNURL metadata is committed to as the invoice's description hash.
    let metadata = r#"[["text/plain","SatsBridge test"]]"#;
    let lnurl = ln
        .create_invoice(
            50,
            &InvoiceDescription {
                memo: "x".into(),
                lnurl_metadata: Some(metadata.into()),
            },
            600,
        )
        .await
        .unwrap();
    assert_eq!(
        decode_invoice(&lnurl.payment_request).unwrap().description_hash,
        Some(sha256::Hash::hash(metadata.as_bytes()).to_string())
    );

    // Unknown invoices read as failed, and expired unpaid ones too.
    assert_eq!(ln.invoice_status(&"0".repeat(64)).await.unwrap(), InvoiceStatus::Failed);
    let short = ln.create_invoice(10, &memo("short"), 1).await.unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert_eq!(ln.invoice_status(&short.payment_hash).await.unwrap(), InvoiceStatus::Failed);
    assert_eq!(sandbox_pay(&base, &short.payment_hash).await, 409);
}

#[tokio::test]
async fn sending_needs_the_admin_key_and_retries_move_no_money() {
    let (_sandbox, base) = start().await;
    let ln = adapter(&base, true, None);

    let invoice = foreign_invoice(100_000);
    let sent = ln.pay_invoice(&invoice, 10).await.unwrap();
    assert_eq!(sent.fee_sats, 1);
    assert!(!sent.preimage.is_empty());

    // Retrying the same invoice returns the existing payment instead of paying again.
    let again = ln.pay_invoice(&invoice, 10).await.unwrap();
    assert_eq!(again.payment_hash, sent.payment_hash);
    let state: serde_json::Value = reqwest::get(format!("{base}/sandbox/state")).await.unwrap().json().await.unwrap();
    // 1,000,000 start - 100 sats - 1 sat fee, once.
    assert_eq!(state["balance_sats"], 999_899);

    // An amount over the balance is refused outright, not left in flight.
    let too_big = foreign_invoice(2_000_000_000_000);
    assert!(matches!(ln.pay_invoice(&too_big, 10).await, Err(AdapterError::Rejected(_))));

    // The invoice key cannot spend, and a wrong key is refused at startup.
    let res = reqwest::Client::new()
        .post(format!("{base}/api/v1/payments"))
        .header("X-API-KEY", INVOICE_KEY)
        .json(&serde_json::json!({ "out": true, "bolt11": foreign_invoice(1_000) }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let wrong = LnbitsAdapter::new(
        reqwest::Client::new(),
        LnbitsConfig {
            base_url: base.clone(),
            invoice_key: "nope".into(),
            admin_key: None,
            webhook_url: None,
        },
    );
    assert!(wrong.verify_credentials().await.is_err());

    // An invoice key passed off as the admin key is caught like on real LNbits.
    let not_admin = LnbitsAdapter::new(
        reqwest::Client::new(),
        LnbitsConfig {
            base_url: base,
            invoice_key: INVOICE_KEY.into(),
            admin_key: Some(INVOICE_KEY.into()),
            webhook_url: None,
        },
    );
    assert!(not_admin.verify_credentials().await.is_err());
}
