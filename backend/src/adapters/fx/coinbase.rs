//! Coinbase public exchange-rates API (no key): `GET /v2/exchange-rates?currency=USD`.

use std::collections::HashMap;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::Deserialize;

use super::{PriceSource, insert_cfa_from_eur, parse_positive};
use crate::{adapters::AdapterError, domain::Currency};

pub const DEFAULT_BASE_URL: &str = "https://api.coinbase.com";

pub struct CoinbaseSource {
    client: reqwest::Client,
    base_url: String,
}

impl CoinbaseSource {
    pub fn new(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into(),
        }
    }
}

#[derive(Deserialize)]
struct Response {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    rates: HashMap<String, String>,
}

pub fn parse(body: &[u8]) -> Result<HashMap<Currency, Decimal>, AdapterError> {
    let rates = serde_json::from_slice::<Response>(body)
        .map_err(|e| AdapterError::Unavailable(format!("unexpected Coinbase response: {e}")))?
        .data
        .rates;
    let rate = |code: &str| rates.get(code).and_then(|r| parse_positive(r));

    let mut prices: HashMap<Currency, Decimal> = [Currency::Gnf, Currency::Kes, Currency::Ngn, Currency::Ghs]
        .into_iter()
        .filter_map(|c| rate(c.code()).map(|p| (c, p)))
        .collect();
    insert_cfa_from_eur(&mut prices, rate("EUR"));
    Ok(prices)
}

#[async_trait]
impl PriceSource for CoinbaseSource {
    fn name(&self) -> &'static str {
        "coinbase"
    }

    async fn fetch(&self) -> Result<HashMap<Currency, Decimal>, AdapterError> {
        let url = format!("{}/v2/exchange-rates", self.base_url.trim_end_matches('/'));
        let body = self
            .client
            .get(url)
            .query(&[("currency", "USD")])
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AdapterError::Unavailable(format!("Coinbase request failed: {e}")))?
            .bytes()
            .await
            .map_err(|e| AdapterError::Unavailable(format!("Coinbase response failed: {e}")))?;
        parse(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rates_and_derives_cfa() {
        let body = br#"{"data":{"currency":"USD","rates":{"EUR":"0.92","NGN":"1549.81","KES":"129.25","GNF":"8650.5","GHS":"15.4","XOF":"1"}}}"#;
        let prices = parse(body).unwrap();
        assert_eq!(prices[&Currency::Ngn].to_string(), "1549.81");
        assert_eq!(prices[&Currency::Gnf].to_string(), "8650.5");
        assert_eq!(prices[&Currency::Ghs].to_string(), "15.4");
        // Feed's own XOF value is ignored in favour of the EUR peg.
        assert_eq!(prices[&Currency::Xof], Decimal::new(60_348_044, 5));
    }

    #[test]
    fn rejects_unexpected_shape() {
        assert!(parse(br#"{"errors":[{"id":"not_found"}]}"#).is_err());
    }
}
