use std::{str::FromStr, sync::Arc, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{
    adapters::{
        collection::CollectionRegistry,
        fx::{CachedFx, CoinGeckoSource, CoinbaseSource, FallbackFx, FxProvider, StaticFxProvider},
        payout::PayoutRegistry,
        stellar::{
            StellarLedger,
            horizon::{HorizonConfig, HorizonLedger},
            simulated::SimulatedLedger,
        },
    },
    config::{CollectionConfig, Config, FxConfig, FxProviderKind, LedgerKind, StellarConfig},
    
    ratelimit::RateLimits,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: SqlitePool,
    pub fx: Arc<dyn FxProvider>,
    pub ledger: Arc<dyn StellarLedger>,
    /// Serializes building and submitting treasury transactions, which share one sequence number.
    pub ledger_lock: Arc<tokio::sync::Mutex<()>>,
    pub collections: Arc<CollectionRegistry>,
    pub payouts: Arc<PayoutRegistry>,
    pub limits: Arc<RateLimits>,
}

impl AppState {
    pub fn assemble(
        config: Config,
        pool: SqlitePool,
        fx: Arc<dyn FxProvider>,
        ledger: Arc<dyn StellarLedger>,
        collections: CollectionRegistry,
        payouts: PayoutRegistry,
    ) -> Self {
        Self {
            limits: Arc::new(RateLimits::new(&config.rate_limit)),
            config: Arc::new(config),
            pool,
            fx,
            ledger,
            ledger_lock: Arc::new(tokio::sync::Mutex::new(())),
            collections: Arc::new(collections),
            payouts: Arc::new(payouts),
        }
    }

    /// Wires adapters from config and checks the Stellar accounts.
    pub async fn from_config(config: Config) -> anyhow::Result<Self> {
        let pool = connect(&config.database_url).await?;
        let fx = build_fx(&config.fx)?;
        let ledger = build_ledger(&config.stellar, fx.clone()).await?;
        let collections = build_collections(&config.collections);
        let payouts = build_payouts().await?;
        Ok(Self::assemble(config, pool, fx, ledger, collections, payouts))
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

pub fn http_client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("stellarbridge/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Builds the provider chain from `FX_PROVIDERS`, e.g. `coinbase,coingecko,static`.
pub fn build_fx(cfg: &FxConfig) -> anyhow::Result<Arc<dyn FxProvider>> {
    let client = http_client(Duration::from_millis(cfg.http_timeout_ms))?;
    let ttl = chrono::Duration::seconds(cfg.cache_ttl_secs);
    let max_age = chrono::Duration::seconds(cfg.max_age_secs);
    let providers: Vec<Arc<dyn FxProvider>> = cfg
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
                FxProviderKind::Static => Arc::new(StaticFxProvider::new(cfg.static_rates.clone())),
            }
        })
        .collect();
    Ok(Arc::new(FallbackFx::new(providers)))
}

/// Connects to Horizon and checks the issuer and treasury accounts, or builds the simulated ledger.
pub async fn build_ledger(cfg: &StellarConfig, fx: Arc<dyn FxProvider>) -> anyhow::Result<Arc<dyn StellarLedger>> {
    match cfg.ledger {
        LedgerKind::Simulated => {
            tracing::warn!("STELLAR_LEDGER=simulated: transactions are simulated in memory, nothing reaches Stellar");
            Ok(Arc::new(SimulatedLedger::new(
                fx,
                cfg.simulated_spread_bps,
                cfg.tx_timeout_secs,
            )))
        }
        LedgerKind::Testnet => {
            let ledger = HorizonLedger::new(
                http_client(Duration::from_secs(45))?,
                HorizonConfig {
                    horizon_url: cfg.horizon_url.clone(),
                    network_passphrase: cfg.network_passphrase.clone(),
                    issuer_secret: cfg.issuer_secret.clone().unwrap_or_default(),
                    treasury_secret: cfg.treasury_secret.clone().unwrap_or_default(),
                    bridge_code: cfg.bridge_code.clone(),
                    tx_timeout_secs: cfg.tx_timeout_secs,
                    explorer_url: cfg.explorer_url.clone(),
                },
            )?;
            ledger
                .verify()
                .await
                .map_err(|e| anyhow::anyhow!("cannot use Stellar at {}: {e}", cfg.horizon_url))?;
            tracing::info!(
                horizon = %cfg.horizon_url,
                issuer = %ledger.bridge_asset().issuer,
                treasury = %ledger.treasury(),
                "connected to Stellar testnet"
            );
            Ok(Arc::new(ledger))
        }
    }
}

/// Collection rails (how senders pay in). Real providers register here; none are wired yet, so
/// no currency is sendable until one is.
pub fn build_collections(_cfg: &CollectionConfig) -> CollectionRegistry {
    CollectionRegistry::default()
}

/// Payout rails. Real providers register here; none are wired yet.
pub async fn build_payouts() -> anyhow::Result<PayoutRegistry> {
    Ok(PayoutRegistry::default())
}
