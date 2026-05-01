//! Postgres connection + migrations.

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn connect(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(database_url)
        .await
        .context("connect to Neon")
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    // Migrations live in `server/migrations/` at the workspace root.
    // The path here is relative to the binary crate.
    sqlx::migrate!("../migrations")
        .run(pool)
        .await
        .context("run migrations")?;
    tracing::info!("migrations applied");
    Ok(())
}
