use std::{collections::HashMap, env, fmt::Display, net::SocketAddr, str::FromStr};

use anyhow::{Context, bail};
use rust_decimal::Decimal;

use crate::{
    adapters::{
        lightning::lnbits::LnbitsConfig,
        payout::bitnob::{self, BitnobConfig},
    },
    domain::Currency,
};

/// Offline fallback prices (approximate, Oct 2026). Only used when `static` is in `FX_PROVIDERS`.
pub const DEMO_FX_PRICES: &str = "XOF=48300000,XAF=48300000,GNF=730000000,KES=10700000,NGN=110000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxProviderKind {
    Coinbase,
    CoinGecko,
    Static,
}

impl FromStr for FxProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "coinbase" => Ok(Self::Coinbase),
            "coingecko" => Ok(Self::CoinGecko),
            "static" => Ok(Self::Static),
            other => Err(format!(
                "unknown FX provider {other:?} (expected coinbase, coingecko or static)"
            )),
        }
    }
}

#[derive(Clone)]
pub struct FxConfig {
    /// Tried in order for each currency.
    pub providers: Vec<FxProviderKind>,
    pub static_prices: HashMap<Currency, Decimal>,
    pub cache_ttl_secs: i64,
    /// Last good prices are served for this long while an upstream is failing.
    pub max_age_secs: i64,
    pub http_timeout_ms: u64,
    pub coinbase_base_url: String,
    pub coingecko_base_url: String,
    pub coingecko_api_key: Option<String>,
}

impl FxConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let providers = env::var("FX_PROVIDERS")
            .unwrap_or_else(|_| "coinbase,coingecko".into())
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|p| p.parse().map_err(anyhow::Error::msg))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if providers.is_empty() {
            bail!("FX_PROVIDERS must list at least one provider");
        }
        Ok(Self {
            providers,
            static_prices: parse_prices(&env::var("FX_STATIC_PRICES").unwrap_or_else(|_| DEMO_FX_PRICES.into()))?,
            cache_ttl_secs: parse_or("FX_CACHE_TTL_SECS", 30)?,
            max_age_secs: parse_or("FX_MAX_AGE_SECS", 300)?,
            http_timeout_ms: parse_or("FX_HTTP_TIMEOUT_MS", 2_000)?,
            coinbase_base_url: env::var("COINBASE_BASE_URL")
                .unwrap_or_else(|_| crate::adapters::fx::coinbase::DEFAULT_BASE_URL.into()),
            coingecko_base_url: env::var("COINGECKO_BASE_URL")
                .unwrap_or_else(|_| crate::adapters::fx::coingecko::DEFAULT_BASE_URL.into()),
            coingecko_api_key: env::var("COINGECKO_API_KEY").ok().filter(|k| !k.is_empty()),
        })
    }

    /// Static prices only; for tests and offline demos.
    pub fn static_only(static_prices: HashMap<Currency, Decimal>) -> Self {
        Self {
            providers: vec![FxProviderKind::Static],
            static_prices,
            cache_ttl_secs: 30,
            max_age_secs: 300,
            http_timeout_ms: 2_000,
            coinbase_base_url: crate::adapters::fx::coinbase::DEFAULT_BASE_URL.into(),
            coingecko_base_url: crate::adapters::fx::coingecko::DEFAULT_BASE_URL.into(),
            coingecko_api_key: None,
        }
    }
}

#[derive(Clone)]
pub struct LightningConfig {
    pub lnbits: LnbitsConfig,
    /// Settlement watcher interval in seconds; 0 disables it (only allowed with a webhook URL).
    pub poll_secs: u64,
}

impl LightningConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let lnbits = LnbitsConfig {
            base_url: required("LNBITS_URL")?,
            invoice_key: required("LNBITS_INVOICE_KEY")?,
            admin_key: env::var("LNBITS_ADMIN_KEY").ok().filter(|k| !k.is_empty()),
            webhook_url: env::var("LNBITS_WEBHOOK_URL").ok().filter(|u| !u.is_empty()),
        };
        let poll_secs = parse_or("LIGHTNING_POLL_SECS", 5)?;
        if poll_secs == 0 && lnbits.webhook_url.is_none() {
            bail!("LIGHTNING_POLL_SECS=0 needs LNBITS_WEBHOOK_URL, or no payment would ever settle");
        }
        Ok(Self { lnbits, poll_secs })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NgnBankProvider {
    Mock,
    Bitnob,
}

#[derive(Clone)]
pub struct PayoutConfig {
    pub ngn_bank_provider: NgnBankProvider,
    pub ngn_max_payout_minor: i64,
    /// Set when `ngn_bank_provider` is Bitnob.
    pub bitnob: Option<BitnobConfig>,
    /// Lightning Address domains for the Tapnob NGN rail; empty disables it.
    pub tapnob_domains: Vec<String>,
}

impl PayoutConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let ngn_max_payout_minor = parse_or("NGN_MAX_PAYOUT_MINOR", 50_000_000)?;
        let ngn_bank_provider = match env::var("NGN_BANK_PROVIDER")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "mock" => NgnBankProvider::Mock,
            "bitnob" => NgnBankProvider::Bitnob,
            other => bail!("unknown NGN_BANK_PROVIDER {other:?} (expected mock or bitnob)"),
        };
        let bitnob = match ngn_bank_provider {
            NgnBankProvider::Mock => None,
            NgnBankProvider::Bitnob => Some(BitnobConfig {
                base_url: env::var("BITNOB_BASE_URL").unwrap_or_else(|_| bitnob::DEFAULT_BASE_URL.into()),
                client_id: required("BITNOB_CLIENT_ID")?,
                client_secret: required("BITNOB_CLIENT_SECRET")?,
                webhook_secret: required("BITNOB_WEBHOOK_SECRET")?,
                callback_url: required("BITNOB_CALLBACK_URL")?,
                from_asset: env::var("BITNOB_FROM_ASSET").unwrap_or_else(|_| "BTC".into()),
                min_payout_minor: parse_or("BITNOB_NGN_MIN_PAYOUT_MINOR", 50_000)?,
                max_payout_minor: ngn_max_payout_minor,
            }),
        };
        let tapnob_domains = env::var("TAPNOB_DOMAINS")
            .unwrap_or_else(|_| "tapnob.io".into())
            .split(',')
            .map(|d| d.trim().to_ascii_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
        Ok(Self {
            ngn_bank_provider,
            ngn_max_payout_minor,
            bitnob,
            tapnob_domains,
        })
    }

    /// Mock NGN bank rail, no Tapnob; for tests and offline demos.
    pub fn mock(ngn_max_payout_minor: i64) -> Self {
        Self {
            ngn_bank_provider: NgnBankProvider::Mock,
            ngn_max_payout_minor,
            bitnob: None,
            tapnob_domains: vec![],
        }
    }
}

/// Deliberately not `Debug`: it holds secrets.
#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub api_key: Option<String>,
    pub provider_webhook_secret: String,
    pub quote_ttl_secs: i64,
    pub invoice_expiry_secs: i64,
    pub resolution_ttl_secs: i64,
    pub fee_bps: i64,
    pub flat_fee_sats: i64,
    pub fx: FxConfig,
    pub payouts: PayoutConfig,
    pub lightning: LightningConfig,
    pub dev_routes: bool,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            bind_addr: parse_or("BIND_ADDR", "0.0.0.0:8080".parse()?)?,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://satsbridge.db".into()),
            api_key: env::var("API_KEY").ok().filter(|k| !k.is_empty()),
            provider_webhook_secret: required("PROVIDER_WEBHOOK_SECRET")?,
            quote_ttl_secs: parse_or("QUOTE_TTL_SECS", 120)?,
            invoice_expiry_secs: parse_or("INVOICE_EXPIRY_SECS", 600)?,
            resolution_ttl_secs: parse_or("RESOLUTION_TTL_SECS", 600)?,
            fee_bps: parse_or("FEE_BPS", 100)?,
            flat_fee_sats: parse_or("FLAT_FEE_SATS", 0)?,
            fx: FxConfig::from_env()?,
            payouts: PayoutConfig::from_env()?,
            lightning: LightningConfig::from_env()?,
            dev_routes: parse_or("DEV_ROUTES", false)?,
        };
        if !(0..10_000).contains(&config.fee_bps) {
            bail!("FEE_BPS must be between 0 and 9999");
        }
        Ok(config)
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("{key} must be set"))
}

fn parse_or<T: FromStr>(key: &str, default: T) -> anyhow::Result<T>
where
    T::Err: Display,
{
    match env::var(key) {
        Ok(raw) if !raw.is_empty() => raw.parse().map_err(|e| anyhow::anyhow!("invalid {key}: {e}")),
        _ => Ok(default),
    }
}

/// Parses `XOF=60000000,NGN=150000000` (major units per 1 BTC).
pub fn parse_prices(raw: &str) -> anyhow::Result<HashMap<Currency, Decimal>> {
    raw.split(',')
        .filter(|pair| !pair.trim().is_empty())
        .map(|pair| {
            let (code, price) = pair
                .split_once('=')
                .with_context(|| format!("invalid price entry {pair:?}"))?;
            let currency = Currency::from_str(code).map_err(anyhow::Error::msg)?;
            let price = Decimal::from_str(price.trim()).with_context(|| format!("invalid price for {code}"))?;
            if price <= Decimal::ZERO {
                bail!("price for {code} must be positive");
            }
            Ok((currency, price))
        })
        .collect()
}
