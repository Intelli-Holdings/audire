//! HTTP + WebSocket routes.

use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

mod health;
mod orgs;
mod sync;
mod users;
mod vaults;

pub fn router(state: AppState) -> Router {
    let public = Router::new().route("/v1/health", get(health::health));

    let authed = Router::new()
        .route("/v1/users/me", post(users::create_or_update_me).get(users::me))
        .route("/v1/users/lookup", get(users::lookup))
        .route("/v1/orgs", get(orgs::list).post(orgs::create))
        .route("/v1/orgs/:id/members", get(orgs::list_members).post(orgs::add_member))
        .route("/v1/vaults", get(vaults::list).post(vaults::create))
        .route("/v1/vaults/:id", get(vaults::get_one))
        .route("/v1/vaults/:id/members", post(vaults::add_member))
        .route("/v1/vaults/:id/rotate-key", post(vaults::rotate_key))
        .route("/v1/sync/:id", get(sync::websocket));

    public
        .merge(authed)
        // 1 MB cap on JSON bodies — op payloads are tiny ciphertext blobs;
        // anything larger is almost certainly a bug or abuse.
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
