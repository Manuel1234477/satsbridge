use std::{str::FromStr, sync::Arc, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{
    adapters::{
        fx::{CachedFx, CoinGeckoSource, CoinbaseSource, FallbackFx, FxProvider, StaticFxProvider},
        lightning::{LightningAdapter, lnbits::LnbitsAdapter},
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
    /// Wires adapters from config and checks the LNbits credentials. Tando is still a mock.
    pub async fn from_config(config: Config) -> anyhow::Result<Self> {
        let pool = connect(&config.database_url).await?;
        let lightning = build_lightning(&config.lightning).await?;
        Ok(Self {
            fx: build_fx(&config.fx)?,
            payouts: Arc::new(build_payouts(&config.payouts, lightning.clone()).await?),
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

/// Builds the payout rails; checks Bitnob credentials when Bitnob is enabled.
pub async fn build_payouts(cfg: &PayoutConfig, lightning: Arc<dyn LightningAdapter>) -> anyhow::Result<PayoutRegistry> {
    let mut payouts = PayoutRegistry::default();
    payouts.register(Arc::new(MockTandoAdapter));
    match (cfg.ngn_bank_provider, &cfg.bitnob) {
        (NgnBankProvider::Bitnob, Some(bitnob)) => {
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
                .build()?;
            let adapter = BitnobAdapter::new(http, bitnob.clone());
            let identity = adapter
                .whoami()
                .await
                .map_err(|e| anyhow::anyhow!("cannot use Bitnob at {}: {e}", bitnob.base_url))?;
            tracing::info!(
                workspace = %identity.client_name.as_deref().unwrap_or("?"),
                environment = %identity.environment.as_deref().unwrap_or("?"),
                "connected to Bitnob"
            );
            if identity.environment.as_deref() == Some("production") {
                tracing::warn!("Bitnob keys are PRODUCTION: payouts send real Naira");
            }
            payouts.register(Arc::new(adapter));
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

/// Connects to LNbits and fails fast on bad URL or keys.
pub async fn build_lightning(cfg: &LightningConfig) -> anyhow::Result<Arc<dyn LightningAdapter>> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let lnbits = LnbitsAdapter::new(http, cfg.lnbits.clone());
    let wallet = lnbits
        .verify_credentials()
        .await
        .map_err(|e| anyhow::anyhow!("cannot use LNbits at {}: {e}", cfg.lnbits.base_url))?;
    tracing::info!(
        url = %cfg.lnbits.base_url,
        wallet = %wallet.name,
        balance_sats = wallet.balance / 1000,
        can_send = cfg.lnbits.admin_key.is_some(),
        "connected to LNbits"
    );
    if cfg.lnbits.admin_key.is_none() {
        tracing::warn!("LNBITS_ADMIN_KEY not set: Lightning Address (Tapnob) payouts will fail");
    }
    Ok(Arc::new(lnbits))
}
