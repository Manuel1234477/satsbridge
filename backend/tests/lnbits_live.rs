//! Live checks against a real LNbits instance. Skipped unless these are set:
//!
//! - `LNBITS_TEST_URL`
//! - `LNBITS_TEST_INVOICE_KEY`, `LNBITS_TEST_ADMIN_KEY`: the wallet under test (needs a balance)
//! - `LNBITS_TEST_PAYEE_INVOICE_KEY`: a second wallet on the same instance that issues the
//!   invoice to pay
//!
//! Run with `cargo test --test lnbits_live -- --nocapture`. Moves real sats on a real node.

use std::time::Duration;

use satsbridge::adapters::lightning::{
    InvoiceDescription, InvoiceStatus, LightningAdapter,
    lnbits::{LnbitsAdapter, LnbitsConfig},
};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn adapter(invoice_key: String, admin_key: Option<String>) -> LnbitsAdapter {
    LnbitsAdapter::new(
        reqwest::Client::new(),
        LnbitsConfig {
            base_url: env("LNBITS_TEST_URL").unwrap(),
            invoice_key,
            admin_key,
            webhook_url: None,
        },
    )
    .with_pay_polling(10, Duration::from_secs(1))
}

#[tokio::test]
async fn receive_and_send_on_a_real_lnbits() {
    let (Some(_), Some(invoice_key), Some(admin_key), Some(payee_key)) = (
        env("LNBITS_TEST_URL"),
        env("LNBITS_TEST_INVOICE_KEY"),
        env("LNBITS_TEST_ADMIN_KEY"),
        env("LNBITS_TEST_PAYEE_INVOICE_KEY"),
    ) else {
        eprintln!("skipping: LNBITS_TEST_* not set");
        return;
    };
    let wallet = adapter(invoice_key, Some(admin_key));
    let payee = adapter(payee_key, None);

    let info = wallet.verify_credentials().await.expect("credentials");
    eprintln!("wallet {:?} balance {} sats", info.name, info.balance / 1000);

    let invoice = payee
        .create_invoice(
            21,
            &InvoiceDescription {
                memo: "satsbridge live test".into(),
                lnurl_metadata: None,
            },
            600,
        )
        .await
        .expect("create invoice");
    assert_eq!(
        payee.invoice_status(&invoice.payment_hash).await.unwrap(),
        InvoiceStatus::Pending
    );

    let sent = wallet
        .pay_invoice(&invoice.payment_request, 10)
        .await
        .expect("pay invoice");
    assert_eq!(sent.payment_hash, invoice.payment_hash);
    eprintln!("paid {} with fee {} sats", sent.payment_hash, sent.fee_sats);

    assert_eq!(
        payee.invoice_status(&invoice.payment_hash).await.unwrap(),
        InvoiceStatus::Paid { amount_sats: 21 }
    );

    // Retrying the same invoice must not move money again. LNbits refuses the duplicate and the
    // adapter reports the existing payment, which is what a payout retry needs.
    let before = wallet.verify_credentials().await.unwrap().balance;
    let again = wallet.pay_invoice(&invoice.payment_request, 10).await.expect("retry");
    assert_eq!(again.payment_hash, invoice.payment_hash);
    assert_eq!(
        wallet.verify_credentials().await.unwrap().balance,
        before,
        "retry moved money"
    );
}
