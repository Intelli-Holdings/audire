//! `AppState` is the immutable handle Axum hands to every route.

use sqlx::PgPool;

use crate::auth::JwksCache;
use crate::config::AppConfig;
use crate::sync_hub::SyncHub;

#[derive(Clone)]
pub struct AppState {
    pub cfg: AppConfig,
    pub db: PgPool,
    pub jwks: JwksCache,
    pub sync_hub: SyncHub,
}

impl AppState {
    pub fn new(cfg: AppConfig, db: PgPool, jwks: JwksCache) -> Self {
        Self {
            cfg,
            db,
            jwks,
            sync_hub: SyncHub::new(),
        }
    }
}
