use std::{collections::HashMap, env, fmt::Display, net::SocketAddr, str::FromStr};

use anyhow::{Context, bail};
use rust_decimal::Decimal;

use crate::{
    adapters::{
        bitnob::{self, BitnobConfig},
        lightning::lnbits::LnbitsConfig,
    },
    domain::{Currency, currency::currency_for_country},
};

/// Offline fallback prices (approximate, Oct 2026). Only used when `static` is in `FX_PROVIDERS`.
pub const DEMO_FX_PRICES: &str = "XOF=48300000,XAF=48300000,GNF=730000000,KES=10700000,NGN=110000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxProviderKind {
    /// Bitnob's payout rate, for currencies with Bitnob corridors.
    Bitnob,
    Coinbase,
    CoinGecko,
    Static,
}

impl FromStr for FxProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bitnob" => Ok(Self::Bitnob),
            "coinbase" => Ok(Self::Coinbase),
            "coingecko" => Ok(Self::CoinGecko),
            "static" => Ok(Self::Static),
            other => Err(format!(
                "unknown FX provider {other:?} (expected bitnob, coinbase, coingecko or static)"
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
    /// Each Bitnob refresh creates throwaway payout quotes, so it refreshes less often.
    pub bitnob_ttl_secs: i64,
    /// Last good prices are served for this long while an upstream is failing.
    pub max_age_secs: i64,
    pub http_timeout_ms: u64,
    pub coinbase_base_url: String,
    pub coingecko_base_url: String,
    pub coingecko_api_key: Option<String>,
}

impl FxConfig {
    /// Defaults to `bitnob,coinbase,coingecko` when Bitnob is configured, so quotes use the rate
    /// Bitnob pays out at; otherwise `coinbase,coingecko`.
    pub fn from_env(bitnob_enabled: bool) -> anyhow::Result<Self> {
        let default = if bitnob_enabled {
            "bitnob,coinbase,coingecko"
        } else {
            "coinbase,coingecko"
        };
        let providers = env::var("FX_PROVIDERS")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| default.into())
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|p| p.parse().map_err(anyhow::Error::msg))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if providers.is_empty() {
            bail!("FX_PROVIDERS must list at least one provider");
        }
        if providers.contains(&FxProviderKind::Bitnob) && !bitnob_enabled {
            bail!("FX_PROVIDERS includes bitnob but BITNOB_COUNTRIES is empty");
        }
        Ok(Self {
            providers,
            static_prices: parse_prices(&env::var("FX_STATIC_PRICES").unwrap_or_else(|_| DEMO_FX_PRICES.into()))?,
            cache_ttl_secs: parse_or("FX_CACHE_TTL_SECS", 30)?,
            bitnob_ttl_secs: parse_or("FX_BITNOB_TTL_SECS", 60)?,
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
            bitnob_ttl_secs: 60,
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
    pub fn from_env(public_base_url: Option<&str>) -> anyhow::Result<Self> {
        let lnbits = LnbitsConfig {
            base_url: required("LNBITS_URL")?,
            invoice_key: required("LNBITS_INVOICE_KEY")?,
            admin_key: env::var("LNBITS_ADMIN_KEY").ok().filter(|k| !k.is_empty()),
            webhook_url: env::var("LNBITS_WEBHOOK_URL")
                .ok()
                .filter(|u| !u.is_empty())
                .or_else(|| public_base_url.map(|b| format!("{b}/api/v1/webhooks/lnbits"))),
        };
        let poll_secs = parse_or("LIGHTNING_POLL_SECS", 5)?;
        if poll_secs == 0 && lnbits.webhook_url.is_none() {
            bail!("LIGHTNING_POLL_SECS=0 needs LNBITS_WEBHOOK_URL, or no payment would ever settle");
        }
        Ok(Self { lnbits, poll_secs })
    }
}

#[derive(Clone)]
pub struct PayoutConfig {
    /// Register mock rails (NGN bank, Tando KES, XOF/XAF mobile money) wherever no real
    /// provider covers that currency and rail.
    pub mock_payouts: bool,
    /// Cap for the mock NGN rail, in kobo.
    pub ngn_max_payout_minor: i64,
    /// Set when `BITNOB_COUNTRIES` is non-empty.
    pub bitnob: Option<BitnobConfig>,
    /// Lightning Address domains for the Tapnob NGN rail; empty disables it.
    pub tapnob_domains: Vec<String>,
}

impl PayoutConfig {
    pub fn from_env(public_base_url: Option<&str>) -> anyhow::Result<Self> {
        let countries: Vec<String> = env::var("BITNOB_COUNTRIES")
            .unwrap_or_default()
            .split(',')
            .map(|c| c.trim().to_ascii_uppercase())
            .filter(|c| !c.is_empty())
            .collect();
        for country in &countries {
            if currency_for_country(country).is_none() {
                bail!("BITNOB_COUNTRIES: {country} is not a supported payout country");
            }
        }
        let bitnob = if countries.is_empty() {
            None
        } else {
            let callback_url = env::var("BITNOB_CALLBACK_URL")
                .ok()
                .filter(|u| !u.is_empty())
                .or_else(|| public_base_url.map(|b| format!("{b}/api/v1/webhooks/bitnob")))
                .context("BITNOB_CALLBACK_URL or PUBLIC_BASE_URL must be set when BITNOB_COUNTRIES is")?;
            Some(BitnobConfig {
                base_url: env::var("BITNOB_BASE_URL").unwrap_or_else(|_| bitnob::DEFAULT_BASE_URL.into()),
                client_id: required("BITNOB_CLIENT_ID")?,
                client_secret: required("BITNOB_CLIENT_SECRET")?,
                webhook_secret: required("BITNOB_WEBHOOK_SECRET")?,
                callback_url,
                from_asset: env::var("BITNOB_FROM_ASSET").unwrap_or_else(|_| "BTC".into()),
                countries,
            })
        };
        let tapnob_domains = env::var("TAPNOB_DOMAINS")
            .unwrap_or_else(|_| "tapnob.io".into())
            .split(',')
            .map(|d| d.trim().to_ascii_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
        Ok(Self {
            mock_payouts: parse_or("MOCK_PAYOUTS", true)?,
            ngn_max_payout_minor: parse_or("NGN_MAX_PAYOUT_MINOR", 50_000_000)?,
            bitnob,
            tapnob_domains,
        })
    }

    /// Mock rails only, no Tapnob; for tests and offline demos.
    pub fn mock(ngn_max_payout_minor: i64) -> Self {
        Self {
            mock_payouts: true,
            ngn_max_payout_minor,
            bitnob: None,
            tapnob_domains: vec![],
        }
    }
}

#[derive(Clone, Copy)]
pub struct RateLimitConfig {
    /// Requests per minute per user on authenticated routes (and per IP on public ones).
    pub per_minute: u32,
    /// New users per hour per IP.
    pub signups_per_hour: u32,
}

#[derive(Clone, Copy)]
pub struct RecoveryConfig {
    /// Recovery worker interval; 0 disables it.
    pub poll_secs: u64,
    /// Submission attempts before a payout goes to manual review.
    pub max_attempts: i64,
    /// Wait before the first retry; doubles per attempt. Must exceed the longest submission.
    pub base_backoff_secs: i64,
    /// Ask the provider about accepted payouts with no webhook after this long.
    pub unconfirmed_after_secs: i64,
}

impl RecoveryConfig {
    pub fn defaults() -> Self {
        Self {
            poll_secs: 30,
            max_attempts: 5,
            base_backoff_secs: 120,
            unconfirmed_after_secs: 600,
        }
    }
}

/// Deliberately not `Debug`: it holds secrets.
#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    /// Public https origin of this server, e.g. `https://pay.example.com`. Used for LNURL and
    /// as the default webhook URLs given to LNbits and Bitnob.
    pub public_base_url: Option<String>,
    /// Bearer token for `/api/v1/admin/*`; admin routes are off when unset.
    pub admin_api_key: Option<String>,
    pub cors_allowed_origins: Vec<String>,
    /// Use `X-Forwarded-For` for client IPs (only behind a trusted proxy).
    pub trust_proxy_headers: bool,
    pub rate_limit: RateLimitConfig,
    pub recovery: RecoveryConfig,
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
        let public_base_url = env::var("PUBLIC_BASE_URL")
            .ok()
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty());
        let payouts = PayoutConfig::from_env(public_base_url.as_deref())?;
        let config = Self {
            bind_addr: parse_or("BIND_ADDR", "0.0.0.0:8080".parse()?)?,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://satsbridge.db".into()),
            admin_api_key: env::var("ADMIN_API_KEY").ok().filter(|k| !k.is_empty()),
            cors_allowed_origins: env::var("CORS_ALLOWED_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(|o| o.trim().trim_end_matches('/').to_string())
                .filter(|o| !o.is_empty())
                .collect(),
            trust_proxy_headers: parse_or("TRUST_PROXY_HEADERS", false)?,
            rate_limit: RateLimitConfig {
                per_minute: parse_or("RATE_LIMIT_PER_MINUTE", 120)?,
                signups_per_hour: parse_or("RATE_LIMIT_SIGNUPS_PER_HOUR", 20)?,
            },
            recovery: RecoveryConfig {
                poll_secs: parse_or("RECOVERY_POLL_SECS", 30)?,
                max_attempts: parse_or("PAYOUT_MAX_ATTEMPTS", 5)?,
                base_backoff_secs: parse_or("PAYOUT_RETRY_BACKOFF_SECS", 120)?,
                unconfirmed_after_secs: parse_or("PAYOUT_UNCONFIRMED_AFTER_SECS", 600)?,
            },
            provider_webhook_secret: required("PROVIDER_WEBHOOK_SECRET")?,
            quote_ttl_secs: parse_or("QUOTE_TTL_SECS", 120)?,
            invoice_expiry_secs: parse_or("INVOICE_EXPIRY_SECS", 600)?,
            resolution_ttl_secs: parse_or("RESOLUTION_TTL_SECS", 600)?,
            fee_bps: parse_or("FEE_BPS", 100)?,
            flat_fee_sats: parse_or("FLAT_FEE_SATS", 0)?,
            fx: FxConfig::from_env(payouts.bitnob.is_some())?,
            lightning: LightningConfig::from_env(public_base_url.as_deref())?,
            payouts,
            public_base_url,
            dev_routes: parse_or("DEV_ROUTES", false)?,
        };
        if let Some(url) = &config.public_base_url
            && !url.starts_with("https://")
        {
            tracing::warn!(%url, "PUBLIC_BASE_URL is not https: Lightning wallets will refuse LNURL from it");
        }
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
