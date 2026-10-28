use std::{str::FromStr, sync::Arc, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{
    adapters::{
        fx::{CachedFx, CoinGeckoSource, CoinbaseSource, FallbackFx, FxProvider, StaticFxProvider},
        lightning::{
            LightningAdapter, MockLightningAdapter,
            lnbits::{LnbitsAdapter, LnbitsConfig},
        },
        payout::{
            PayoutRegistry, bitnob::BitnobAdapter, lightning_address::LightningAddressAdapter,
            ngn_mock::MockNgnAdapter, tando_mock::MockTandoAdapter,
        },
    },
    config::{Config, FxConfig, FxProviderKind, LightningConfig, NgnBankProvider, PayoutConfig},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: SqlitePool,
    pub fx: Arc<dyn FxProvider>,
    pub lightning: Arc<dyn LightningAdapter>,
    pub payouts: Arc<PayoutRegistry>,
}

impl AppState {
    /// Wires adapters from config. Tando is still a mock.
    pub async fn from_config(config: Config) -> anyhow::Result<Self> {
        let pool = connect(&config.database_url).await?;
        let lightning = build_lightning(&config.lightning)?;
        Ok(Self {
            fx: build_fx(&config.fx)?,
            payouts: Arc::new(build_payouts(&config.payouts, lightning.clone())?),
            lightning,
            config: Arc::new(config),
            pool,
        })
    }
}

pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Builds the provider chain from `FX_PROVIDERS`, e.g. `coinbase,coingecko[,static]`.
pub fn build_fx(cfg: &FxConfig) -> anyhow::Result<Arc<dyn FxProvider>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cfg.http_timeout_ms))
        .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let ttl = chrono::Duration::seconds(cfg.cache_ttl_secs);
    let max_age = chrono::Duration::seconds(cfg.max_age_secs);
    let providers = cfg
        .providers
        .iter()
        .map(|kind| -> Arc<dyn FxProvider> {
            match kind {
                FxProviderKind::Coinbase => Arc::new(CachedFx::new(
                    CoinbaseSource::new(client.clone(), &cfg.coinbase_base_url),
                    ttl,
                    max_age,
                )),
                FxProviderKind::CoinGecko => Arc::new(CachedFx::new(
                    CoinGeckoSource::new(client.clone(), &cfg.coingecko_base_url, cfg.coingecko_api_key.clone()),
                    ttl,
                    max_age,
                )),
                FxProviderKind::Static => Arc::new(StaticFxProvider::new(cfg.static_prices.clone())),
            }
        })
        .collect();
    Ok(Arc::new(FallbackFx::new(providers)))
}

pub fn build_payouts(cfg: &PayoutConfig, lightning: Arc<dyn LightningAdapter>) -> anyhow::Result<PayoutRegistry> {
    let mut payouts = PayoutRegistry::default();
    payouts.register(Arc::new(MockTandoAdapter));
    match (cfg.ngn_bank_provider, &cfg.bitnob) {
        (NgnBankProvider::Bitnob, Some(bitnob)) => {
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
                .build()?;
            payouts.register(Arc::new(BitnobAdapter::new(http, bitnob.clone())));
        }
        (NgnBankProvider::Bitnob, None) => anyhow::bail!("NGN_BANK_PROVIDER=bitnob needs Bitnob credentials"),
        (NgnBankProvider::Mock, _) => payouts.register(Arc::new(MockNgnAdapter::new(cfg.ngn_max_payout_minor))),
    }
    if !cfg.tapnob_domains.is_empty() {
        // No redirects: LNURL hosts come from user input, so only allowlisted hosts are contacted.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
            .build()?;
        payouts.register(Arc::new(LightningAddressAdapter::tapnob(
            http,
            lightning,
            cfg.tapnob_domains.clone(),
        )));
    }
    Ok(payouts)
}

pub fn build_lightning(cfg: &LightningConfig) -> anyhow::Result<Arc<dyn LightningAdapter>> {
    let Some(lnbits) = &cfg.lnbits else {
        return Ok(Arc::new(MockLightningAdapter));
    };
    let LnbitsConfig { base_url, .. } = lnbits;
    tracing::info!(%base_url, "using LNbits Lightning node");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
        .build()?;
    Ok(Arc::new(LnbitsAdapter::new(http, lnbits.clone())))
}
