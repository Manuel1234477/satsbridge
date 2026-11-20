use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;

use super::{FxPrice, FxProvider};
use crate::{adapters::AdapterError, domain::Currency};

/// Fixed USD rates from configuration, for offline demos and tests.
pub struct StaticFxProvider {
    prices: HashMap<Currency, Decimal>,
}

impl StaticFxProvider {
    pub fn new(prices: HashMap<Currency, Decimal>) -> Self {
        Self { prices }
    }
}

#[async_trait]
impl FxProvider for StaticFxProvider {
    fn name(&self) -> &'static str {
        "static-config"
    }

    async fn usd_rate(&self, currency: Currency) -> Result<FxPrice, AdapterError> {
        let rate = self
            .prices
            .get(&currency)
            .copied()
            .ok_or_else(|| AdapterError::Unavailable(format!("no static USD rate for {currency}")))?;
        Ok(FxPrice {
            rate,
            source: self.name().to_string(),
            as_of: Utc::now(),
        })
    }
}
