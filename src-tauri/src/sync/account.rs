//! Local persistence for the optional cloud sync account.
//!
//! There is exactly one row at most: `id = 1`. Mode is `local_only`
//! (no row) or `cloud` (row present). The wrapped private key + KEK
//! salt + recovery envelope all live here so the user can re-derive the
//! KEK on every launch without ever uploading the passphrase.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{ParaError, Result};
use crate::store::db::LocalStore;
use crate::sync::crypto::{
    aead_open, aead_seal, derive_kek, mint_recovery_envelope, new_identity_keypair, random_bytes,
    KekMaterial,
};
use x25519_dalek::{PublicKey, StaticSecret};

/// What the UI shows in the Account panel. No secrets here — public
/// info only.
#[derive(Debug, Clone, Serialize)]
pub struct AccountStatus {
    pub mode: String,
    pub server_url: Option<String>,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub public_key_hex: Option<String>,
    pub last_synced_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SignUpRequest {
    pub server_url: String,
    pub email: String,
    pub access_token: String,
    pub passphrase: String,
}

#[derive(Debug, Deserialize)]
pub struct SignInRequest {
    pub server_url: String,
    pub email: String,
    pub access_token: String,
    pub passphrase: String,
}

#[derive(Debug, Serialize)]
pub struct RecoveryReveal {
    pub recovery_hex: String,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sync_account(
    id INTEGER PRIMARY KEY CHECK (id = 1),
    server_url TEXT NOT NULL,
    email TEXT NOT NULL,
    user_id TEXT NOT NULL,
    public_key BLOB NOT NULL,
    kek_salt BLOB NOT NULL,
    wrapped_private_key BLOB NOT NULL,
    wrapped_kek_for_recovery BLOB NOT NULL,
    access_token TEXT,
    last_synced_ms INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
"#;

/// Run once at startup so the optional table exists even if the user
/// has never signed in. Idempotent.
pub fn ensure_schema(store: &LocalStore) -> Result<()> {
    store
        .with_conn(|c| c.execute_batch(SCHEMA))
        .map_err(|e| ParaError::Db(format!("sync schema: {e}")))?;
    Ok(())
}

pub fn account_status(store: &LocalStore) -> Result<AccountStatus> {
    ensure_schema(store)?;
    let row: Option<(String, String, String, Vec<u8>, Option<i64>)> = store
        .with_conn(|c| {
            c.query_row(
                "SELECT server_url, email, user_id, public_key, last_synced_ms FROM sync_account WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
        })
        .map_err(|e| ParaError::Db(format!("read sync_account: {e}")))?;

    Ok(match row {
        None => AccountStatus {
            mode: "local_only".into(),
            server_url: None,
            email: None,
            user_id: None,
            public_key_hex: None,
            last_synced_ms: None,
        },
        Some((server_url, email, user_id, public_key, last_synced_ms)) => AccountStatus {
            mode: "cloud".into(),
            server_url: Some(server_url),
            email: Some(email),
            user_id: Some(user_id),
            public_key_hex: Some(hex::encode(public_key)),
            last_synced_ms,
        },
    })
}

/// First-time sign-up: derive a KEK from the passphrase, generate a
/// fresh identity keypair, build the recovery envelope, register with
/// the server, and persist locally. Returns the one-time recovery key
/// hex string — surface it to the user once and never store it.
pub async fn sign_up(store: &LocalStore, req: &SignUpRequest) -> Result<RecoveryReveal> {
    ensure_schema(store)?;

    let salt = random_bytes(16).map_err(|e| ParaError::Other(e.to_string()))?;
    let kek = derive_kek(&req.passphrase, &salt).map_err(|e| ParaError::Other(e.to_string()))?;

    let (secret, public) = new_identity_keypair().map_err(|e| ParaError::Other(e.to_string()))?;
    let secret_bytes = secret.to_bytes();
    let wrapped_priv = aead_seal(&kek.0, &secret_bytes, b"audire-identity v1")
        .map_err(|e| ParaError::Other(e.to_string()))?;

    let (recovery_hex, wrapped_kek) =
        mint_recovery_envelope(&kek).map_err(|e| ParaError::Other(e.to_string()))?;

    // Talk to the server first so we don't persist a half-account on
    // failure.
    let resp = crate::sync::client::SyncClient::new(&req.server_url, &req.access_token)
        .register_user(&req.email, public.as_bytes(), &wrapped_kek)
        .await
        .map_err(|e| ParaError::Other(format!("register failed: {e}")))?;

    let now = chrono::Utc::now().timestamp_millis();
    let pub_bytes = public.as_bytes().to_vec();
    let server_url = req.server_url.clone();
    let email = req.email.clone();
    let user_id = resp.user_id;
    let access_token = req.access_token.clone();

    store
        .with_conn(move |c| {
            c.execute(
                r#"INSERT OR REPLACE INTO sync_account
                   (id, server_url, email, user_id, public_key, kek_salt,
                    wrapped_private_key, wrapped_kek_for_recovery, access_token,
                    last_synced_ms, created_at, updated_at)
                   VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?9)"#,
                params![
                    server_url,
                    email,
                    user_id,
                    pub_bytes,
                    salt,
                    wrapped_priv,
                    wrapped_kek,
                    access_token,
                    now,
                ],
            )
        })
        .map_err(|e| ParaError::Db(format!("insert sync_account: {e}")))?;

    Ok(RecoveryReveal { recovery_hex })
}

/// Second-device sign-in is intentionally stubbed for v1.
///
/// The full multi-device flow (re-wrap private key for the new device's
/// local KEK using the server-stored recovery envelope) lives in
/// `docs/cloud-architecture.md` §5. For v1 the user has two ways onto
/// a second device:
///   1. Use the recovery key on the second device once we ship the
///      "restore from recovery" UI in the next desktop release.
///   2. Until then, sign-in errors with a clear explanation here so the
///      UI can surface the right message instead of silently failing.
pub async fn sign_in(_store: &LocalStore, _req: &SignInRequest) -> Result<()> {
    Err(ParaError::InvalidState(
        "multi-device sign-in arrives in the next desktop release".into(),
    ))
}

pub fn sign_out(store: &LocalStore) -> Result<()> {
    ensure_schema(store)?;
    store
        .with_conn(|c| c.execute("DELETE FROM sync_account WHERE id = 1", []))
        .map_err(|e| ParaError::Db(format!("delete sync_account: {e}")))?;
    Ok(())
}

/// Decrypt the local identity private key using the in-memory KEK.
/// Used when wrapping a vault key for a new member.
pub fn unwrap_identity_secret(
    store: &LocalStore,
    kek: &KekMaterial,
) -> Result<(StaticSecret, PublicKey)> {
    let row: Option<(Vec<u8>, Vec<u8>)> = store
        .with_conn(|c| {
            c.query_row(
                "SELECT wrapped_private_key, public_key FROM sync_account WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
        .map_err(|e| ParaError::Db(format!("read sync_account: {e}")))?;
    let (wrapped, public_bytes) = row
        .ok_or_else(|| ParaError::InvalidState("not signed in to cloud sync".into()))?;
    let pt = aead_open(&kek.0, &wrapped, b"audire-identity v1")
        .map_err(|e| ParaError::Other(format!("identity unwrap: {e}")))?;
    if pt.len() != 32 {
        return Err(ParaError::Other("identity key has wrong length".into()));
    }
    let mut sk_bytes = [0u8; 32];
    sk_bytes.copy_from_slice(&pt);
    let secret = StaticSecret::from(sk_bytes);
    if public_bytes.len() != 32 {
        return Err(ParaError::Other("public_key has wrong length".into()));
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&public_bytes);
    let public = PublicKey::from(pk);
    if PublicKey::from(&secret).as_bytes() != public.as_bytes() {
        return Err(ParaError::InvalidState(
            "passphrase did not unwrap the identity key".into(),
        ));
    }
    Ok((secret, public))
}
