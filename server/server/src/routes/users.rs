//! `/v1/users/*` endpoints.

use audire_shared::{CreateUserRequest, UserLookupResponse, UserProfile};
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{AuthCtx, PreSignupAuth};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `POST /v1/users/me` — first-sign-in handshake.
///
/// The client has just signed in to Stack Auth, derived a KEK from the
/// user's passphrase, generated an X25519 keypair, and minted a recovery
/// key envelope. This endpoint is idempotent: calling it again with the
/// same `public_key` is a no-op; with a different `public_key` it 409s.
pub async fn create_or_update_me(
    State(state): State<AppState>,
    auth: PreSignupAuth,
    Json(body): Json<CreateUserRequest>,
) -> ApiResult<Json<UserProfile>> {
    if body.public_key.len() != 32 {
        return Err(ApiError::BadRequest(
            "public_key must be 32 bytes (X25519)".into(),
        ));
    }
    let recovery_id = Uuid::new_v4();

    let mut tx = state.db.begin().await?;

    // Insert (or fetch existing) user row.
    let user_row = sqlx::query!(
        r#"
        INSERT INTO audire.users (id, email, public_key, recovery_key_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (id) DO UPDATE
            SET email = EXCLUDED.email,
                updated_at = now()
        RETURNING id, email, public_key, recovery_key_id, created_at
        "#,
        auth.stack_user_id,
        auth.email,
        &body.public_key,
        recovery_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    // If the existing public_key on file differs, refuse — the user must
    // explicitly rotate via a separate endpoint we'll add when needed.
    if user_row.public_key != body.public_key {
        return Err(ApiError::Conflict(
            "public_key mismatch with existing profile; passphrase rotation not yet supported".into(),
        ));
    }

    // Insert (or update) recovery envelope.
    sqlx::query!(
        r#"
        INSERT INTO audire.recovery_keys (id, user_id, wrapped_kek)
        VALUES ($1, $2, $3)
        ON CONFLICT (id) DO UPDATE SET wrapped_kek = EXCLUDED.wrapped_kek
        "#,
        recovery_id,
        auth.stack_user_id,
        &body.wrapped_kek_for_recovery,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(UserProfile {
        id: user_row.id,
        email: user_row.email,
        public_key: user_row.public_key,
        recovery_key_id: user_row.recovery_key_id,
        created_at: user_row.created_at,
    }))
}

/// `GET /v1/users/me` — return the caller's profile.
pub async fn me(State(state): State<AppState>, auth: AuthCtx) -> ApiResult<Json<UserProfile>> {
    let row = sqlx::query!(
        r#"SELECT id, email, public_key, recovery_key_id, created_at
           FROM audire.users WHERE id = $1"#,
        auth.user_id
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(UserProfile {
        id: row.id,
        email: row.email,
        public_key: row.public_key,
        recovery_key_id: row.recovery_key_id,
        created_at: row.created_at,
    }))
}

#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    pub email: String,
}

/// `GET /v1/users/lookup?email=...` — fetch another user's public key so
/// the caller can wrap a vault key for them. Only resolves users who
/// already exist in our DB (i.e. who have signed up to Audire Sync).
///
/// TODO(phase-2): per-user rate limit (10 lookups / minute) to prevent
/// email enumeration.
pub async fn lookup(
    State(state): State<AppState>,
    _auth: AuthCtx,
    Query(q): Query<LookupQuery>,
) -> ApiResult<Json<UserLookupResponse>> {
    let row = sqlx::query!(
        r#"SELECT id, email, public_key
           FROM audire.users
           WHERE lower(email) = lower($1)"#,
        q.email
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::NotFound("no audire user with that email".into()))?;

    Ok(Json(UserLookupResponse {
        id: row.id,
        email: row.email,
        public_key: row.public_key,
    }))
}
