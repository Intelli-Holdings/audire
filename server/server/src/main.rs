//! Audire Sync server — entry point.
//!
//! Wiring is intentionally thin: parse env, build the `AppState`, build
//! the Axum `Router`, bind, run. Every meaningful unit lives in its own
//! module so it stays testable without spinning up the whole HTTP stack.

#![forbid(unsafe_code)]

use std::net::SocketAddr;

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod auth;
mod config;
mod db;
mod error;
mod routes;
mod state;
mod sync_hub;

use config::AppConfig;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    install_tracing();

    let cfg = AppConfig::from_env().context("loading config")?;
    tracing::info!(bind = %cfg.bind_addr, "starting audire-server");

    let pool = db::connect(&cfg.database_url).await?;
    db::migrate(&pool).await?;

    let jwks = auth::JwksCache::new(cfg.stack_auth_jwks_url.clone());
    let state = AppState::new(cfg.clone(), pool, jwks);

    let app = routes::router(state);
    let addr: SocketAddr = cfg.bind_addr.parse().context("bind addr parse")?;
    let listener = TcpListener::bind(addr).await.context("bind")?;
    tracing::info!(addr = %addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;
    Ok(())
}

fn install_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("audire_server=info,tower_http=info,sqlx=warn")
        }))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            tracing::error!(error = %e, "ctrl_c handler failed");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }
}
