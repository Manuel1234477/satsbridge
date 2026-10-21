use std::{collections::BTreeMap, str::FromStr, sync::Arc, time::Duration};

use rust_decimal::Decimal;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{
    adapters::{
        bitnob::{BitnobClient, payout::BitnobPayoutAdapter, rates::BitnobRateSource},
        fx::{CachedFx, CoinGeckoSource, CoinbaseSource, FallbackFx, FxProvider, StaticFxProvider},
        lightning::{LightningAdapter, lnbits::LnbitsAdapter},
        payout::{
            PayoutRegistry, lightning_address::LightningAddressAdapter, mobile_money_mock::MockMobileMoneyAdapter,
            ngn_mock::MockNgnAdapter, tando_mock::MockTandoAdapter,
        },
    },
    config::{Config, FxConfig, FxProviderKind, LightningConfig, PayoutConfig},
    domain::{Currency, Rail, currency::currency_for_country},
    ratelimit::RateLimits,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: SqlitePool,
    pub fx: Arc<dyn FxProvider>,
    pub lightning: Arc<dyn LightningAdapter>,
    pub payouts: Arc<PayoutRegistry>,
    pub limits: Arc<RateLimits>,
}

/// Bitnob client plus the rate probes for the corridors it serves.
pub struct BitnobRates {
    pub client: Arc<BitnobClient>,
    pub probes: Vec<(Currency, String, Decimal)>,
}

impl AppState {
    pub fn assemble(
        config: Config,
        pool: SqlitePool,
        fx: Arc<dyn FxProvider>,
        lightning: Arc<dyn LightningAdapter>,
        payouts: PayoutRegistry,
    ) -> Self {
        Self {
            limits: Arc::new(RateLimits::new(&config.rate_limit)),
            config: Arc::new(config),
            pool,
            fx,
            lightning,
            payouts: Arc::new(payouts),
        }
    }

    /// Wires adapters from config and checks LNbits and Bitnob credentials.
    pub async fn from_config(config: Config) -> anyhow::Result<Self> {
        let pool = connect(&config.database_url).await?;
        let lightning = build_lightning(&config.lightning).await?;
        let (payouts, bitnob) = build_payouts(&config.payouts, lightning.clone()).await?;
        let fx = build_fx(&config.fx, bitnob)?;
        Ok(Self::assemble(config, pool, fx, lightning, payouts))
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

fn http_client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("satsbridge/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Builds the provider chain from `FX_PROVIDERS`, e.g. `bitnob,coinbase,coingecko[,static]`.
pub fn build_fx(cfg: &FxConfig, bitnob: Option<BitnobRates>) -> anyhow::Result<Arc<dyn FxProvider>> {
    let client = http_client(Duration::from_millis(cfg.http_timeout_ms))?;
    let ttl = chrono::Duration::seconds(cfg.cache_ttl_secs);
    let max_age = chrono::Duration::seconds(cfg.max_age_secs);
    let mut bitnob = bitnob;
    let mut providers: Vec<Arc<dyn FxProvider>> = Vec::new();
    for kind in &cfg.providers {
        providers.push(match kind {
            FxProviderKind::Bitnob => {
                let rates = bitnob
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("FX provider bitnob needs BITNOB_COUNTRIES"))?;
                Arc::new(CachedFx::new(
                    BitnobRateSource::new(rates.client, rates.probes),
                    chrono::Duration::seconds(cfg.bitnob_ttl_secs),
                    max_age,
                ))
            }
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
        });
    }
    Ok(Arc::new(FallbackFx::new(providers)))
}

/// Builds the payout rails. Bitnob corridors are checked against Bitnob at startup (credentials,
/// limits, field rules); mocks fill in where no real provider serves a currency and rail.
pub async fn build_payouts(
    cfg: &PayoutConfig,
    lightning: Arc<dyn LightningAdapter>,
) -> anyhow::Result<(PayoutRegistry, Option<BitnobRates>)> {
    let mut payouts = PayoutRegistry::default();
    let mut bitnob_rates = None;

    if let Some(bitnob) = &cfg.bitnob {
        let client = Arc::new(BitnobClient::new(http_client(Duration::from_secs(15))?, bitnob.clone()));
        let fail = |e| anyhow::anyhow!("cannot use Bitnob at {}: {e}", bitnob.base_url);
        let identity = client.whoami().await.map_err(fail)?;
        tracing::info!(
            workspace = %identity.client_name.as_deref().unwrap_or("?"),
            environment = %identity.environment.as_deref().unwrap_or("?"),
            countries = ?bitnob.countries,
            "connected to Bitnob"
        );
        if identity.environment.as_deref() == Some("production") {
            tracing::warn!("Bitnob keys are PRODUCTION: payouts send real money");
        }
        let limits = client.limits().await.map_err(fail)?;

        let mut corridors: BTreeMap<(Currency, &'static str), (Rail, Vec<String>)> = BTreeMap::new();
        for country in &bitnob.countries {
            let currency = currency_for_country(country).expect("validated in config");
            let rail = if country == "NG" {
                Rail::NgBankTransfer
            } else {
                Rail::MobileMoney
            };
            corridors
                .entry((currency, rail.as_str()))
                .or_insert((rail, Vec::new()))
                .1
                .push(country.clone());
        }
        let mut probes = Vec::new();
        for ((currency, _), (rail, countries)) in corridors {
            let adapter = BitnobPayoutAdapter::connect(client.clone(), currency, rail, &countries, &limits)
                .await
                .map_err(fail)?;
            probes.extend(adapter.rate_probes());
            payouts.register(Arc::new(adapter));
        }
        bitnob_rates = Some(BitnobRates { client, probes });
    }

    if cfg.mock_payouts {
        if payouts.get(Currency::Ngn, Rail::NgBankTransfer).is_none() {
            payouts.register(Arc::new(MockNgnAdapter::new(cfg.ngn_max_payout_minor)));
        }
        payouts.register(Arc::new(MockTandoAdapter));
        for currency in [Currency::Xof, Currency::Xaf] {
            if payouts.get(currency, Rail::MobileMoney).is_none() {
                payouts.register(Arc::new(MockMobileMoneyAdapter::new(currency)));
            }
        }
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
    Ok((payouts, bitnob_rates))
}

/// Connects to LNbits and fails fast on bad URL or keys.
pub async fn build_lightning(cfg: &LightningConfig) -> anyhow::Result<Arc<dyn LightningAdapter>> {
    let lnbits = LnbitsAdapter::new(http_client(Duration::from_secs(30))?, cfg.lnbits.clone());
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
        tracing::warn!("LNBITS_ADMIN_KEY not set: Lightning Address (Tapnob) payouts and refunds will fail");
    }
    Ok(Arc::new(lnbits))
}
