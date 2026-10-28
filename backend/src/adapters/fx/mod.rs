//! BTC price feeds. Live sources are wrapped in [`CachedFx`] and chained with [`FallbackFx`].
//!
//! These are reference rates (BTC/USD or BTC/EUR crossed with fiat FX), not executable prices.
//! Once a real off-ramp is connected, the destination leg should come from that provider's quote.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use tokio::sync::Mutex;

use super::AdapterError;
use crate::domain::Currency;

pub mod coinbase;
pub mod coingecko;
pub mod static_rates;

pub use coinbase::CoinbaseSource;
pub use coingecko::CoinGeckoSource;
pub use static_rates::StaticFxProvider;

/// XOF (BCEAO) and XAF (BEAC) are both fixed at 655.957 per EUR.
pub fn cfa_per_eur() -> Decimal {
    Decimal::new(655_957, 3)
}

#[derive(Clone, Debug)]
pub struct FxPrice {
    /// Major units of the currency per 1 BTC.
    pub price: Decimal,
    /// Recorded on the quote so the rate used is auditable.
    pub source: String,
    pub as_of: DateTime<Utc>,
}

#[async_trait]
pub trait FxProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn btc_price(&self, currency: Currency) -> Result<FxPrice, AdapterError>;
}

/// An upstream API that returns every price it supports in one request.
#[async_trait]
pub trait PriceSource: Send + Sync {
    fn name(&self) -> &'static str;

    async fn fetch(&self) -> Result<HashMap<Currency, Decimal>, AdapterError>;
}

/// Adds the CFA franc prices derived from BTC/EUR.
pub fn insert_cfa_from_eur(prices: &mut HashMap<Currency, Decimal>, btc_eur: Option<Decimal>) {
    if let Some(eur) = btc_eur {
        prices.insert(Currency::Xof, eur * cfa_per_eur());
        prices.insert(Currency::Xaf, eur * cfa_per_eur());
    }
}

pub fn parse_positive(raw: &str) -> Option<Decimal> {
    let raw = raw.trim();
    raw.parse::<Decimal>()
        .ok()
        .or_else(|| Decimal::from_scientific(raw).ok())
        .filter(|d| *d > Decimal::ZERO)
}

struct CacheState {
    prices: HashMap<Currency, Decimal>,
    fetched_at: Option<DateTime<Utc>>,
    last_attempt: Option<DateTime<Utc>>,
}

/// Fetches at most once per `ttl` (successful or not), and keeps serving the last good prices
/// for up to `max_age` while the upstream is failing.
pub struct CachedFx<S> {
    source: S,
    ttl: Duration,
    max_age: Duration,
    state: Mutex<CacheState>,
}

impl<S: PriceSource> CachedFx<S> {
    pub fn new(source: S, ttl: Duration, max_age: Duration) -> Self {
        Self {
            source,
            ttl,
            max_age,
            state: Mutex::new(CacheState {
                prices: HashMap::new(),
                fetched_at: None,
                last_attempt: None,
            }),
        }
    }
}

#[async_trait]
impl<S: PriceSource> FxProvider for CachedFx<S> {
    fn name(&self) -> &'static str {
        self.source.name()
    }

    async fn btc_price(&self, currency: Currency) -> Result<FxPrice, AdapterError> {
        let name = self.source.name();
        // Held across the fetch so concurrent quotes share one upstream request.
        let mut state = self.state.lock().await;
        let now = Utc::now();
        let within = |at: Option<DateTime<Utc>>, window: Duration| at.is_some_and(|t| now - t < window);

        if !within(state.fetched_at, self.ttl) && !within(state.last_attempt, self.ttl) {
            state.last_attempt = Some(now);
            match self.source.fetch().await {
                Ok(prices) => {
                    state.prices = prices;
                    state.fetched_at = Some(now);
                }
                Err(err) => tracing::warn!(source = name, error = %err, "BTC price refresh failed"),
            }
        }

        if !within(state.fetched_at, self.max_age) {
            return Err(AdapterError::Unavailable(format!(
                "{name} prices are unavailable or stale"
            )));
        }
        let as_of = state.fetched_at.expect("checked by within()");
        state
            .prices
            .get(&currency)
            .map(|&price| FxPrice {
                price,
                source: name.to_string(),
                as_of,
            })
            .ok_or_else(|| AdapterError::Unavailable(format!("{name} has no BTC price for {currency}")))
    }
}

/// Tries each provider in order, per currency.
pub struct FallbackFx {
    providers: Vec<Arc<dyn FxProvider>>,
}

impl FallbackFx {
    pub fn new(providers: Vec<Arc<dyn FxProvider>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl FxProvider for FallbackFx {
    fn name(&self) -> &'static str {
        "fallback"
    }

    async fn btc_price(&self, currency: Currency) -> Result<FxPrice, AdapterError> {
        let mut last_err = None;
        for provider in &self.providers {
            match provider.btc_price(currency).await {
                Ok(price) => return Ok(price),
                Err(err) => {
                    tracing::warn!(provider = provider.name(), %currency, error = %err, "FX provider failed; trying next");
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| AdapterError::Unavailable("no FX providers configured".into())))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FakeSource {
        calls: AtomicUsize,
        fail_after: usize,
    }

    #[async_trait]
    impl PriceSource for FakeSource {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn fetch(&self) -> Result<HashMap<Currency, Decimal>, AdapterError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n >= self.fail_after {
                return Err(AdapterError::Unavailable("down".into()));
            }
            Ok(HashMap::from([(Currency::Ngn, Decimal::from(110_000_000))]))
        }
    }

    fn fake(fail_after: usize) -> FakeSource {
        FakeSource {
            calls: AtomicUsize::new(0),
            fail_after,
        }
    }

    #[tokio::test]
    async fn caches_within_ttl() {
        let fx = CachedFx::new(fake(10), Duration::seconds(60), Duration::seconds(300));
        fx.btc_price(Currency::Ngn).await.unwrap();
        fx.btc_price(Currency::Ngn).await.unwrap();
        assert_eq!(fx.source.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn serves_last_good_prices_while_upstream_fails() {
        let fx = CachedFx::new(fake(1), Duration::zero(), Duration::seconds(300));
        fx.btc_price(Currency::Ngn).await.unwrap();
        let price = fx.btc_price(Currency::Ngn).await.unwrap();
        assert_eq!(price.price, Decimal::from(110_000_000));
        assert_eq!(fx.source.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rejects_stale_prices() {
        let fx = CachedFx::new(fake(1), Duration::zero(), Duration::zero());
        assert!(fx.btc_price(Currency::Ngn).await.is_err());
    }

    #[tokio::test]
    async fn falls_back_per_currency() {
        let primary: Arc<dyn FxProvider> =
            Arc::new(CachedFx::new(fake(10), Duration::seconds(60), Duration::seconds(60)));
        let backup: Arc<dyn FxProvider> = Arc::new(StaticFxProvider::new(HashMap::from([(
            Currency::Kes,
            Decimal::from(10_700_000),
        )])));
        let fx = FallbackFx::new(vec![primary, backup]);
        assert_eq!(fx.btc_price(Currency::Ngn).await.unwrap().source, "fake");
        assert_eq!(fx.btc_price(Currency::Kes).await.unwrap().source, "static-config");
        assert!(fx.btc_price(Currency::Gnf).await.is_err());
    }

    #[test]
    fn derives_cfa_from_eur_peg() {
        let mut prices = HashMap::new();
        insert_cfa_from_eur(&mut prices, Some(Decimal::from(70_000)));
        assert_eq!(prices[&Currency::Xof], Decimal::new(45_916_990, 0));
        assert_eq!(prices[&Currency::Xof], prices[&Currency::Xaf]);
    }
}
