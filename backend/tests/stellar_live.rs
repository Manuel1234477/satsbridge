//! Checks against real Stellar testnet using the `STELLAR_*` keys in `.env`. Ignored by default;
//! run with `cargo test --test stellar_live -- --ignored --nocapture` after `stellar_setup`.
//!
//! Verifies the issuer and treasury accounts and trustlines, then asks Horizon for NGN -> every
//! destination path. Read-only: submits no transactions.

use std::time::Duration;

use stellarbridge::{
    adapters::stellar::{
        STROOPS_PER_UNIT, StellarLedger, TESTNET_HORIZON, TESTNET_PASSPHRASE,
        horizon::{HorizonConfig, HorizonLedger},
    },
    domain::Currency,
    state::http_client,
};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::test]
#[ignore = "live: calls Stellar testnet; run with `-- --ignored`"]
async fn testnet_accounts_and_dex_paths() {
    dotenvy::dotenv().ok();
    let (Some(issuer), Some(treasury)) = (env("STELLAR_ISSUER_SECRET"), env("STELLAR_TREASURY_SECRET")) else {
        eprintln!("skipping: STELLAR_ISSUER_SECRET / STELLAR_TREASURY_SECRET not set");
        return;
    };
    let ledger = HorizonLedger::new(
        http_client(Duration::from_secs(30)).unwrap(),
        HorizonConfig {
            horizon_url: env("STELLAR_HORIZON_URL").unwrap_or_else(|| TESTNET_HORIZON.into()),
            network_passphrase: TESTNET_PASSPHRASE.into(),
            issuer_secret: issuer,
            treasury_secret: treasury,
            bridge_code: env("STELLAR_BRIDGE_ASSET").unwrap_or_else(|| "USD".into()),
            tx_timeout_secs: 60,
            explorer_url: None,
        },
    )
    .unwrap();
    ledger
        .verify()
        .await
        .expect("accounts and trustlines (run stellar_setup)");

    // ₦100,000 into every other currency.
    let amount = 100_000 * STROOPS_PER_UNIT;
    for destination in Currency::ALL.into_iter().filter(|c| *c != Currency::Ngn) {
        let quote = ledger.quote_send(Currency::Ngn, amount, destination).await.unwrap();
        println!(
            "NGN 100000 -> {destination} {} via {:?}",
            rust_decimal::Decimal::new(quote.destination_amount, 7).normalize(),
            quote.hops
        );
        assert!(quote.destination_amount > 0);
        assert_eq!(quote.hops.first().map(String::as_str), Some("NGN"));
    }
}
