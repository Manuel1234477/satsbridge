//! Read-only checks against the real Bitnob API using the `BITNOB_*` values in `.env`.
//! Skipped unless `BITNOB_CLIENT_ID` and `BITNOB_CLIENT_SECRET` are set. Moves no money and
//! creates no quotes.
//!
//! For each country in `BITNOB_COUNTRIES` (default `NG`) it prints the payout limits, the bank or
//! operator directory, and the beneficiary fields Bitnob requires, which is how to confirm the
//! mobile money field mapping. Run with `cargo test --test bitnob_live -- --nocapture`.

use satsbridge::{
    adapters::bitnob::{BitnobClient, BitnobConfig, DEFAULT_BASE_URL},
    domain::currency::currency_for_country,
};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::test]
#[ignore = "live: calls the real Bitnob API; run with `-- --ignored`"]
async fn bitnob_credentials_corridors_and_fields() {
    dotenvy::dotenv().ok();
    let (Some(client_id), Some(client_secret)) = (env("BITNOB_CLIENT_ID"), env("BITNOB_CLIENT_SECRET")) else {
        eprintln!("skipping: BITNOB_CLIENT_ID / BITNOB_CLIENT_SECRET not set");
        return;
    };
    let countries: Vec<String> = env("BITNOB_COUNTRIES")
        .unwrap_or_else(|| "NG".into())
        .split(',')
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| !c.is_empty())
        .collect();
    let bitnob = BitnobClient::new(
        reqwest::Client::new(),
        BitnobConfig {
            base_url: env("BITNOB_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into()),
            client_id,
            client_secret,
            webhook_secret: String::new(),
            callback_url: String::new(),
            from_asset: "BTC".into(),
            countries: countries.clone(),
        },
    );

    let me = bitnob
        .whoami()
        .await
        .expect("whoami: check client id, secret and system clock");
    eprintln!("workspace {:?}, environment {:?}", me.client_name, me.environment);
    let limits = bitnob.limits().await.expect("limits");

    for country in &countries {
        let currency = currency_for_country(country).expect("supported country");
        eprintln!("\n== {country} ({currency})");
        match limits
            .iter()
            .find(|l| l.country.eq_ignore_ascii_case(country) && l.currency == currency.code())
        {
            Some(l) => eprintln!("limits: {} .. {} {currency}", l.min_amount, l.max_amount),
            None => eprintln!("NO LIMITS: this corridor isn't enabled on the workspace"),
        }
        let banks = bitnob.banks(country).await.expect("bank/operator directory");
        let sample: Vec<_> = banks
            .iter()
            .take(5)
            .map(|b| format!("{} = {}", b.bank_name.trim(), b.bank_code))
            .collect();
        eprintln!("{} banks/operators, e.g. {sample:?}", banks.len());
        let requirements = bitnob.requirements(country).await.expect("country requirements");
        if let Some(types) = requirements.get("destination_types").and_then(|t| t.as_object()) {
            for (kind, spec) in types {
                let fields: Vec<String> = spec
                    .get("fields")
                    .and_then(|f| f.as_array())
                    .into_iter()
                    .flatten()
                    .map(|f| {
                        format!(
                            "{}{}{}",
                            f["key"].as_str().unwrap_or("?"),
                            if f["required"].as_bool() == Some(true) { "*" } else { "" },
                            f["pattern"].as_str().map(|p| format!(" /{p}/")).unwrap_or_default()
                        )
                    })
                    .collect();
                eprintln!("{kind} fields (* = required): {fields:?}");
            }
        }
    }
}
