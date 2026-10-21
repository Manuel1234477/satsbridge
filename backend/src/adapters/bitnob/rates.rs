//! The rate Bitnob actually pays out at. Bitnob's payout quote says how many sats a given
//! settlement amount costs (fees included), so a quote for a typical amount gives an executable
//! BTC price per currency. Each probe creates a short-lived Bitnob quote that is never used;
//! wrap this source in `CachedFx` so that happens at most once per cache period.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rust_decimal::Decimal;

use super::BitnobClient;
use crate::{
    adapters::{AdapterError, fx::PriceSource},
    domain::{Currency, money::SATS_PER_BTC},
};

pub struct BitnobRateSource {
    client: Arc<BitnobClient>,
    /// (currency, country, settlement amount in major units)
    probes: Vec<(Currency, String, Decimal)>,
}

impl BitnobRateSource {
    pub fn new(client: Arc<BitnobClient>, probes: Vec<(Currency, String, Decimal)>) -> Self {
        Self { client, probes }
    }
}

/// Major units per BTC implied by "`sats` buys `settlement` units".
pub fn price_from_quote(settlement: Decimal, sats: i64) -> Option<Decimal> {
    (sats > 0).then(|| settlement * Decimal::from(SATS_PER_BTC) / Decimal::from(sats))
}

#[async_trait]
impl PriceSource for BitnobRateSource {
    fn name(&self) -> &'static str {
        "bitnob"
    }

    async fn fetch(&self) -> Result<HashMap<Currency, Decimal>, AdapterError> {
        let mut prices = HashMap::new();
        let mut last_err = None;
        for (currency, country, amount) in &self.probes {
            let reference = format!("rate-{}", uuid::Uuid::new_v4().simple());
            let settlement = amount.normalize().to_string();
            match self
                .client
                .quote(currency.code(), country, &settlement, "rate check", &reference)
                .await
            {
                Ok(quote) => match quote.sats().and_then(|sats| price_from_quote(*amount, sats)) {
                    Some(price) => {
                        prices.insert(*currency, price);
                    }
                    None => tracing::warn!(%currency, "Bitnob quote had no sat amount"),
                },
                Err(err) => {
                    tracing::warn!(%currency, error = %err, "Bitnob rate probe failed");
                    last_err = Some(err);
                }
            }
        }
        match (prices.is_empty(), last_err) {
            (true, Some(err)) => Err(err),
            _ => Ok(prices),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::fake::*, *};

    #[test]
    fn derives_price_from_quote() {
        // 100,000 NGN for 90,909 sats -> ~110M NGN per BTC.
        let price = price_from_quote(Decimal::from(100_000), 90_909).unwrap();
        assert_eq!(price.round(), Decimal::from(110_000_110));
        assert!(price_from_quote(Decimal::from(1), 0).is_none());
    }

    #[tokio::test]
    async fn probes_each_currency() {
        let base = serve(Fake::default()).await;
        let client = Arc::new(BitnobClient::new(reqwest::Client::new(), config(base, SECRET, &["NG"])));
        let source = BitnobRateSource::new(client, vec![(Currency::Ngn, "NG".into(), Decimal::from(100_000))]);
        // The fake prices 1 sat per unit: 1e8 NGN per BTC.
        assert_eq!(
            source.fetch().await.unwrap()[&Currency::Ngn],
            Decimal::from(SATS_PER_BTC)
        );
    }
}
