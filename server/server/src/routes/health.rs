use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::ApiResult;
use crate::state::AppState;

/// Liveness + readiness in one. Returns 200 with a small JSON blob if
/// the process is up and the DB pool can answer a `SELECT 1`.
pub async fn health(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await?;
    Ok(Json(json!({
        "status": "ok",
        "service": "audire-server",
        "version": env!("CARGO_PKG_VERSION"),
    })))
}
