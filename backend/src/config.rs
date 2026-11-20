use std::{collections::HashMap, env, fmt::Display, net::SocketAddr, str::FromStr};

use anyhow::{Context, bail};
use rust_decimal::Decimal;

use crate::{adapters::stellar, domain::Currency};

/// Offline fallback rates (units per 1 USD, approximate, Nov 2026). Used by the `static`
/// provider in `FX_PROVIDERS`, and by default for the simulated ledger.
pub const DEMO_FX_RATES: &str = "NGN=1550,KES=129,GHS=15.4,XOF=603.5,XAF=603.5,GNF=8650";

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
    pub static_rates: HashMap<Currency, Decimal>,
    pub cache_ttl_secs: i64,
    /// Last good rates are served for this long while an upstream is failing.
    pub max_age_secs: i64,
    pub http_timeout_ms: u64,
    pub coinbase_base_url: String,
    pub coingecko_base_url: String,
    pub coingecko_api_key: Option<String>,
}

impl FxConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let providers = env::var("FX_PROVIDERS")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "coinbase,coingecko,static".into())
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|p| p.parse().map_err(anyhow::Error::msg))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if providers.is_empty() {
            bail!("FX_PROVIDERS must list at least one provider");
        }
        Ok(Self {
            providers,
            static_rates: parse_rates(&env::var("FX_STATIC_RATES").unwrap_or_else(|_| DEMO_FX_RATES.into()))?,
            cache_ttl_secs: parse_or("FX_CACHE_TTL_SECS", 60)?,
            max_age_secs: parse_or("FX_MAX_AGE_SECS", 600)?,
            http_timeout_ms: parse_or("FX_HTTP_TIMEOUT_MS", 2_000)?,
            coinbase_base_url: env::var("COINBASE_BASE_URL")
                .unwrap_or_else(|_| crate::adapters::fx::coinbase::DEFAULT_BASE_URL.into()),
            coingecko_base_url: env::var("COINGECKO_BASE_URL")
                .unwrap_or_else(|_| crate::adapters::fx::coingecko::DEFAULT_BASE_URL.into()),
            coingecko_api_key: env::var("COINGECKO_API_KEY").ok().filter(|k| !k.is_empty()),
        })
    }

    /// Static rates only; for tests and offline demos.
    pub fn static_only(static_rates: HashMap<Currency, Decimal>) -> Self {
        Self {
            providers: vec![FxProviderKind::Static],
            static_rates,
            cache_ttl_secs: 60,
            max_age_secs: 600,
            http_timeout_ms: 2_000,
            coinbase_base_url: crate::adapters::fx::coinbase::DEFAULT_BASE_URL.into(),
            coingecko_base_url: crate::adapters::fx::coingecko::DEFAULT_BASE_URL.into(),
            coingecko_api_key: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    /// Stellar testnet through Horizon.
    Testnet,
    /// In-memory ledger priced from the FX feed; nothing touches the network.
    Simulated,
}

/// Deliberately not `Debug`: it holds secret keys.
#[derive(Clone)]
pub struct StellarConfig {
    pub ledger: LedgerKind,
    pub horizon_url: String,
    pub network_passphrase: String,
    pub issuer_secret: Option<String>,
    pub treasury_secret: Option<String>,
    /// Asset code paths route through.
    pub bridge_code: String,
    /// How long a signed transaction stays valid; also the wait before an unconfirmed one is rebuilt.
    pub tx_timeout_secs: i64,
    pub explorer_url: Option<String>,
    /// DEX spread per hop for the simulated ledger.
    pub simulated_spread_bps: i64,
}

impl StellarConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let ledger = match env::var("STELLAR_LEDGER")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "testnet" => LedgerKind::Testnet,
            "simulated" => LedgerKind::Simulated,
            other => bail!("STELLAR_LEDGER must be testnet or simulated, not {other:?}"),
        };
        let secret = |key: &str| {
            env::var(key)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let config = Self {
            ledger,
            horizon_url: env::var("STELLAR_HORIZON_URL").unwrap_or_else(|_| stellar::TESTNET_HORIZON.into()),
            network_passphrase: env::var("STELLAR_NETWORK_PASSPHRASE")
                .unwrap_or_else(|_| stellar::TESTNET_PASSPHRASE.into()),
            issuer_secret: secret("STELLAR_ISSUER_SECRET"),
            treasury_secret: secret("STELLAR_TREASURY_SECRET"),
            bridge_code: env::var("STELLAR_BRIDGE_ASSET").unwrap_or_else(|_| "USD".into()),
            tx_timeout_secs: parse_or("STELLAR_TX_TIMEOUT_SECS", 60)?,
            explorer_url: env::var("STELLAR_EXPLORER_URL")
                .ok()
                .or_else(|| Some("https://stellar.expert/explorer/testnet".into()))
                .filter(|u| !u.is_empty()),
            simulated_spread_bps: parse_or("SIMULATED_SPREAD_BPS", 30)?,
        };
        if config.network_passphrase != stellar::TESTNET_PASSPHRASE {
            bail!("only Stellar testnet is supported (STELLAR_NETWORK_PASSPHRASE)");
        }
        if ledger == LedgerKind::Testnet && (config.issuer_secret.is_none() || config.treasury_secret.is_none()) {
            bail!(
                "STELLAR_ISSUER_SECRET and STELLAR_TREASURY_SECRET must be set (run `cargo run --bin stellar_setup`), \
                 or set STELLAR_LEDGER=simulated"
            );
        }
        Ok(config)
    }

    pub fn simulated() -> Self {
        Self {
            ledger: LedgerKind::Simulated,
            horizon_url: stellar::TESTNET_HORIZON.into(),
            network_passphrase: stellar::TESTNET_PASSPHRASE.into(),
            issuer_secret: None,
            treasury_secret: None,
            bridge_code: "USD".into(),
            tx_timeout_secs: 60,
            explorer_url: None,
            simulated_spread_bps: 30,
        }
    }
}

#[derive(Clone, Copy)]
pub struct CollectionConfig {
    /// Register the mock NGN virtual-account collection rail.
    pub mock_collections: bool,
    /// Cap for one NGN deposit, in kobo.
    pub ngn_max_deposit_minor: i64,
    /// How long the sender has to make the bank transfer.
    pub deposit_expiry_secs: i64,
}

#[derive(Clone)]
pub struct PayoutConfig {
    /// Register mock rails (NGN bank, Tando KES, mobile money) wherever no real provider covers
    /// that currency and rail. The NGN bank rail also carries refunds to senders.
    pub mock_payouts: bool,
    /// Cap for the mock NGN rail, in kobo.
    pub ngn_max_payout_minor: i64,
}

impl PayoutConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            mock_payouts: parse_or("MOCK_PAYOUTS", true)?,
            ngn_max_payout_minor: parse_or("NGN_MAX_PAYOUT_MINOR", 50_000_000)?,
        })
    }

    /// Mock rails only; for tests and offline demos.
    pub fn mock(ngn_max_payout_minor: i64) -> Self {
        Self {
            mock_payouts: true,
            ngn_max_payout_minor,
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
    /// Look up Stellar transactions with no recorded outcome after this long.
    pub stellar_confirm_after_secs: i64,
}

impl RecoveryConfig {
    pub fn defaults() -> Self {
        Self {
            poll_secs: 10,
            max_attempts: 5,
            base_backoff_secs: 120,
            unconfirmed_after_secs: 600,
            stellar_confirm_after_secs: 15,
        }
    }
}

/// Deliberately not `Debug`: it holds secrets.
#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    /// Public https origin of this server, e.g. `https://pay.example.com`.
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
    pub resolution_ttl_secs: i64,
    pub fee_bps: i64,
    /// Flat fee in source-currency minor units.
    pub flat_fee_minor: i64,
    /// Held back from the DEX estimate so the quoted payout survives small price moves.
    pub slippage_bps: i64,
    pub fx: FxConfig,
    pub stellar: StellarConfig,
    pub collections: CollectionConfig,
    pub payouts: PayoutConfig,
    pub dev_routes: bool,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let public_base_url = env::var("PUBLIC_BASE_URL")
            .ok()
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty());
        let payouts = PayoutConfig::from_env()?;
        let config = Self {
            bind_addr: parse_or("BIND_ADDR", "0.0.0.0:8080".parse()?)?,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://stellarbridge.db".into()),
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
                poll_secs: parse_or("RECOVERY_POLL_SECS", 10)?,
                max_attempts: parse_or("PAYOUT_MAX_ATTEMPTS", 5)?,
                base_backoff_secs: parse_or("PAYOUT_RETRY_BACKOFF_SECS", 120)?,
                unconfirmed_after_secs: parse_or("PAYOUT_UNCONFIRMED_AFTER_SECS", 600)?,
                stellar_confirm_after_secs: parse_or("STELLAR_CONFIRM_AFTER_SECS", 15)?,
            },
            provider_webhook_secret: required("PROVIDER_WEBHOOK_SECRET")?,
            quote_ttl_secs: parse_or("QUOTE_TTL_SECS", 120)?,
            resolution_ttl_secs: parse_or("RESOLUTION_TTL_SECS", 600)?,
            fee_bps: parse_or("FEE_BPS", 100)?,
            flat_fee_minor: parse_or("FLAT_FEE_MINOR", 0)?,
            slippage_bps: parse_or("SLIPPAGE_BPS", 50)?,
            fx: FxConfig::from_env()?,
            stellar: StellarConfig::from_env()?,
            collections: CollectionConfig {
                mock_collections: parse_or("MOCK_COLLECTIONS", true)?,
                ngn_max_deposit_minor: parse_or("NGN_MAX_DEPOSIT_MINOR", 500_000_000)?,
                deposit_expiry_secs: parse_or("DEPOSIT_EXPIRY_SECS", 1_800)?,
            },
            payouts,
            public_base_url,
            dev_routes: parse_or("DEV_ROUTES", false)?,
        };
        if !(0..10_000).contains(&config.fee_bps) {
            bail!("FEE_BPS must be between 0 and 9999");
        }
        if !(0..5_000).contains(&config.slippage_bps) {
            bail!("SLIPPAGE_BPS must be between 0 and 4999");
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

/// Parses `NGN=1550,KES=129` (major units per 1 USD).
pub fn parse_rates(raw: &str) -> anyhow::Result<HashMap<Currency, Decimal>> {
    raw.split(',')
        .filter(|pair| !pair.trim().is_empty())
        .map(|pair| {
            let (code, price) = pair
                .split_once('=')
                .with_context(|| format!("invalid rate entry {pair:?}"))?;
            let currency = Currency::from_str(code).map_err(anyhow::Error::msg)?;
            let price = Decimal::from_str(price.trim()).with_context(|| format!("invalid rate for {code}"))?;
            if price <= Decimal::ZERO {
                bail!("rate for {code} must be positive");
            }
            Ok((currency, price))
        })
        .collect()
}
