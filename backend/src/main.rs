use std::{net::SocketAddr, time::Duration};

use satsbridge::{AppState, Config, router, services};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "satsbridge=info,tower_http=info".into()))
        .init();

    let config = Config::from_env()?;
    let bind_addr = config.bind_addr;
    if config.dev_routes {
        tracing::warn!("DEV_ROUTES enabled: the mock payout simulator is mounted. Do not use in production.");
    }
    if config.admin_api_key.is_none() {
        tracing::info!("ADMIN_API_KEY not set: admin routes are disabled");
    }

    let state = AppState::from_config(config).await?;
    if state.config.lightning.poll_secs > 0 {
        services::settlement::spawn_watcher(state.clone(), Duration::from_secs(state.config.lightning.poll_secs));
    }
    if state.config.recovery.poll_secs > 0 {
        services::recovery::spawn_worker(state.clone(), Duration::from_secs(state.config.recovery.poll_secs));
    }
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "SatsBridge API listening");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
