//! Read-only checks against the real Bitnob API using the `BITNOB_*` values in `.env`.
//! Skipped unless `BITNOB_CLIENT_ID` and `BITNOB_CLIENT_SECRET` are set. Moves no money.
//!
//! Run with `cargo test --test bitnob_live -- --nocapture`.

use satsbridge::adapters::payout::{
    PayoutAdapter,
    bitnob::{BitnobAdapter, BitnobConfig, DEFAULT_BASE_URL},
};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::test]
async fn bitnob_credentials_and_bank_list() {
    dotenvy::dotenv().ok();
    let (Some(client_id), Some(client_secret)) = (env("BITNOB_CLIENT_ID"), env("BITNOB_CLIENT_SECRET")) else {
        eprintln!("skipping: BITNOB_CLIENT_ID / BITNOB_CLIENT_SECRET not set");
        return;
    };
    let bitnob = BitnobAdapter::new(
        reqwest::Client::new(),
        BitnobConfig {
            base_url: env("BITNOB_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into()),
            client_id,
            client_secret,
            webhook_secret: String::new(),
            callback_url: String::new(),
            from_asset: "BTC".into(),
            min_payout_minor: 50_000,
            max_payout_minor: 50_000_000,
        },
    );

    let me = bitnob.whoami().await.expect("whoami: check client id, secret and system clock");
    eprintln!("workspace {:?}, environment {:?}", me.client_name, me.environment);

    let banks = bitnob.institutions().await.expect("bank list");
    assert!(!banks.is_empty());
    eprintln!("{} Nigerian banks, e.g. {} = {}", banks.len(), banks[0].name, banks[0].code);
}
