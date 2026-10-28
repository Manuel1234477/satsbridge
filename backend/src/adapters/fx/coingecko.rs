//! CoinGecko simple-price API: `GET /api/v3/simple/price?ids=bitcoin&vs_currencies=...`.
//! Covers EUR (→ XOF/XAF), NGN and KES; GNF is not offered, so it falls through to the next provider.

use std::collections::HashMap;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde_json::Value;

use super::{PriceSource, insert_cfa_from_eur, parse_positive};
use crate::{adapters::AdapterError, domain::Currency};

pub const DEFAULT_BASE_URL: &str = "https://api.coingecko.com";

pub struct CoinGeckoSource {
    client: reqwest::Client,
    base_url: String,
    /// Optional demo-plan key, sent as `x-cg-demo-api-key`.
    api_key: Option<String>,
}

impl CoinGeckoSource {
    pub fn new(client: reqwest::Client, base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            client,
            base_url: base_url.into(),
            api_key,
        }
    }
}

pub fn parse(body: &[u8]) -> Result<HashMap<Currency, Decimal>, AdapterError> {
    let json: Value = serde_json::from_slice(body)
        .map_err(|e| AdapterError::Unavailable(format!("unexpected CoinGecko response: {e}")))?;
    let btc = json
        .get("bitcoin")
        .and_then(Value::as_object)
        .ok_or_else(|| AdapterError::Unavailable("CoinGecko response has no bitcoin prices".into()))?;
    let rate = |code: &str| {
        btc.get(code)
            .filter(|v| v.is_number())
            .and_then(|v| parse_positive(&v.to_string()))
    };

    let mut prices: HashMap<Currency, Decimal> = [Currency::Gnf, Currency::Kes, Currency::Ngn]
        .into_iter()
        .filter_map(|c| rate(&c.code().to_ascii_lowercase()).map(|p| (c, p)))
        .collect();
    insert_cfa_from_eur(&mut prices, rate("eur"));
    Ok(prices)
}

#[async_trait]
impl PriceSource for CoinGeckoSource {
    fn name(&self) -> &'static str {
        "coingecko"
    }

    async fn fetch(&self) -> Result<HashMap<Currency, Decimal>, AdapterError> {
        let url = format!("{}/api/v3/simple/price", self.base_url.trim_end_matches('/'));
        let mut req = self
            .client
            .get(url)
            .query(&[("ids", "bitcoin"), ("vs_currencies", "eur,ngn,kes,gnf")]);
        if let Some(key) = &self.api_key {
            req = req.header("x-cg-demo-api-key", key);
        }
        let body = req
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AdapterError::Unavailable(format!("CoinGecko request failed: {e}")))?
            .bytes()
            .await
            .map_err(|e| AdapterError::Unavailable(format!("CoinGecko response failed: {e}")))?;
        parse(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_and_skips_missing_currencies() {
        let prices = parse(br#"{"bitcoin":{"eur":70000,"ngn":109926307.5,"kes":10732716}}"#).unwrap();
        assert_eq!(prices[&Currency::Ngn].to_string(), "109926307.5");
        assert_eq!(prices[&Currency::Kes], Decimal::from(10_732_716));
        assert_eq!(prices[&Currency::Xaf], Decimal::from(45_916_990));
        assert!(!prices.contains_key(&Currency::Gnf));
    }

    #[test]
    fn rejects_error_body() {
        assert!(parse(br#"{"status":{"error_code":429}}"#).is_err());
    }
}
