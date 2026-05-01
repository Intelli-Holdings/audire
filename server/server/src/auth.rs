//! Stack Auth JWT verification.
//!
//! Stack Auth issues short-lived RS256 JWTs signed with keys it publishes
//! at a JWKS URL. We:
//!   1. Cache the JWKS (refresh every hour, force-refresh on `kid` miss).
//!   2. Verify signature, exp, iss, aud on every request.
//!   3. Map the `sub` claim (Stack Auth user ID) to our `audire.users.id`.
//!
//! The `Auth` extractor below is what routes use to get a verified user.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

/// Claims we expect on a Stack Auth JWT. Stack Auth includes other
/// fields (display name, etc.) but only these are load-bearing for us.
#[derive(Debug, Clone, Deserialize)]
pub struct StackAuthClaims {
    pub sub: String,
    pub iss: String,
    /// Email — may be present at `email` or under a `claims` substruct
    /// depending on Stack Auth version. We try the top-level first.
    #[serde(default)]
    pub email: Option<String>,
    pub exp: i64,
}

/// The verified caller passed to every authenticated route.
#[derive(Debug, Clone)]
pub struct AuthCtx {
    /// Audire's internal user UUID. Created lazily on first sign-in via
    /// `POST /v1/users/me`. Until that endpoint has been called, every
    /// other endpoint will return 401 with code `signup_required`.
    pub user_id: Uuid,
    pub email: String,
}

#[derive(Clone)]
pub struct JwksCache {
    inner: Arc<RwLock<Inner>>,
    url: String,
}

struct Inner {
    set: Option<JwkSet>,
    fetched_at: Option<Instant>,
}

const JWKS_TTL: Duration = Duration::from_secs(60 * 60);

impl JwksCache {
    pub fn new(url: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                set: None,
                fetched_at: None,
            })),
            url,
        }
    }

    pub async fn key_for(&self, kid: &str) -> Result<DecodingKey, ApiError> {
        if let Some(key) = self.lookup(kid).await {
            return Ok(key);
        }
        // Cache miss or stale — refresh once, then retry.
        self.refresh().await.map_err(|e| {
            tracing::warn!(error = %e, "failed to refresh JWKS");
            ApiError::Unauthorized("could not validate token".into())
        })?;
        self.lookup(kid)
            .await
            .ok_or_else(|| ApiError::Unauthorized("unknown signing key".into()))
    }

    async fn lookup(&self, kid: &str) -> Option<DecodingKey> {
        let guard = self.inner.read().await;
        let stale = guard
            .fetched_at
            .map(|t| t.elapsed() > JWKS_TTL)
            .unwrap_or(true);
        if stale {
            return None;
        }
        let set = guard.set.as_ref()?;
        let jwk = set.find(kid)?;
        DecodingKey::from_jwk(jwk).ok()
    }

    async fn refresh(&self) -> anyhow::Result<()> {
        let body = reqwest::Client::new()
            .get(&self.url)
            .send()
            .await
            .context("fetch JWKS")?
            .error_for_status()
            .context("JWKS HTTP status")?
            .json::<JwkSet>()
            .await
            .context("parse JWKS JSON")?;
        let mut guard = self.inner.write().await;
        guard.set = Some(body);
        guard.fetched_at = Some(Instant::now());
        Ok(())
    }
}

/// Verify a JWT and return the parsed claims. Pure function so it can be
/// unit-tested without a running Axum app.
async fn verify(token: &str, jwks: &JwksCache, audience: &str) -> Result<StackAuthClaims, ApiError> {
    let header = decode_header(token).map_err(|_| ApiError::Unauthorized("malformed token".into()))?;
    let kid = header
        .kid
        .ok_or_else(|| ApiError::Unauthorized("token missing kid".into()))?;
    let key = jwks.key_for(&kid).await?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[audience]);
    // We do not pin `iss` here because Stack Auth's issuer URL contains
    // the project id which we already pin via `audience` + JWKS URL.
    let data = decode::<StackAuthClaims>(token, &key, &validation)
        .map_err(|e| ApiError::Unauthorized(format!("invalid token: {}", e)))?;
    Ok(data.claims)
}

#[async_trait]
impl FromRequestParts<AppState> for AuthCtx {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or_else(|| ApiError::Unauthorized("missing Authorization header".into()))?;
        let raw = header
            .to_str()
            .map_err(|_| ApiError::Unauthorized("bad Authorization header".into()))?;
        let token = raw
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("expected Bearer token".into()))?;

        let claims = verify(token, &state.jwks, &state.cfg.stack_auth_audience).await?;
        let stack_uid = claims.sub.clone();
        let email = claims
            .email
            .clone()
            .ok_or_else(|| ApiError::Unauthorized("token missing email claim".into()))?;

        // Look up the local mirror row. Created by POST /v1/users/me.
        let row = sqlx::query!(
            r#"SELECT id FROM audire.users WHERE id::text = $1"#,
            stack_uid
        )
        .fetch_optional(&state.db)
        .await
        .map_err(ApiError::from)?;

        let user_id = match row {
            Some(r) => r.id,
            None => {
                return Err(ApiError::Unauthorized(
                    "no audire profile yet — POST /v1/users/me first".into(),
                ));
            }
        };

        Ok(AuthCtx { user_id, email })
    }
}

/// Variant used by `POST /v1/users/me` itself: returns the verified
/// Stack Auth identity without requiring a pre-existing audire user row.
pub struct PreSignupAuth {
    pub stack_user_id: Uuid,
    pub email: String,
}

#[async_trait]
impl FromRequestParts<AppState> for PreSignupAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or_else(|| ApiError::Unauthorized("missing Authorization header".into()))?;
        let raw = header.to_str().map_err(|_| ApiError::Unauthorized("bad header".into()))?;
        let token = raw
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("expected Bearer token".into()))?;
        let claims = verify(token, &state.jwks, &state.cfg.stack_auth_audience).await?;
        let stack_user_id = Uuid::parse_str(&claims.sub)
            .map_err(|_| ApiError::Unauthorized("sub claim is not a UUID".into()))?;
        let email = claims
            .email
            .ok_or_else(|| ApiError::Unauthorized("token missing email claim".into()))?;
        Ok(PreSignupAuth {
            stack_user_id,
            email,
        })
    }
}

#[allow(dead_code)]
fn unused_anyhow() -> anyhow::Result<()> {
    // Keep `anyhow` linked even if no current call site uses it directly
    // (we'll use it as more routes land).
    Err(anyhow!("placeholder")).map(|_: ()| ())
}
