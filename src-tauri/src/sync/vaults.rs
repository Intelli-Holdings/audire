//! Local persistence for vault membership + key material.
//!
//! `sync_vaults` is the desktop-side cache of `GET /v1/vaults`. The
//! `wrapped_vault_key` column contains the vault key sealed for the
//! local user's identity keypair, so unwrapping requires the
//! in-memory KEK + the unwrapped private key from `sync_account`.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::error::{ParaError, Result};
use crate::store::db::LocalStore;
use crate::sync::account::unwrap_identity_secret;
use crate::sync::crypto::{open_sealed, KekMaterial, VaultKey};

#[derive(Debug, Clone, Serialize)]
pub struct LocalVaultRow {
    pub vault_id: String,
    pub name: String,
    pub role: String,
    pub org_id: Option<String>,
    pub last_op_id_applied: i64,
    pub last_op_id_remote: i64,
}

pub fn list(store: &LocalStore) -> Result<Vec<LocalVaultRow>> {
    let rows: Vec<LocalVaultRow> = store
        .with_conn(|c| {
            let mut stmt = c.prepare(
                r#"SELECT vault_id, name, role, org_id, last_op_id_applied, last_op_id_remote
                   FROM sync_vaults ORDER BY created_at ASC"#,
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(LocalVaultRow {
                        vault_id: r.get(0)?,
                        name: r.get(1)?,
                        role: r.get(2)?,
                        org_id: r.get(3)?,
                        last_op_id_applied: r.get(4)?,
                        last_op_id_remote: r.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .map_err(|e| ParaError::Db(format!("list sync_vaults: {e}")))?;
    Ok(rows)
}

/// Persist a vault row, replacing any prior copy. Called when the
/// caller creates a vault locally (knows the unwrapped key already) or
/// receives an invite (via `GET /v1/vaults` — the wrapped_vault_key
/// will be the sealed-box variant in that case).
pub fn upsert(
    store: &LocalStore,
    vault_id: &str,
    name: &str,
    role: &str,
    org_id: Option<&str>,
    wrapped_vault_key: &[u8],
    last_op_id_remote: i64,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let vault_id = vault_id.to_string();
    let name = name.to_string();
    let role = role.to_string();
    let org_id = org_id.map(|s| s.to_string());
    let wrapped = wrapped_vault_key.to_vec();
    store
        .with_conn(move |c| {
            c.execute(
                r#"INSERT INTO sync_vaults
                       (vault_id, name, role, wrapped_vault_key, org_id,
                        last_op_id_applied, last_op_id_remote, created_at, updated_at)
                       VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?7)
                   ON CONFLICT(vault_id) DO UPDATE SET
                       name = excluded.name,
                       role = excluded.role,
                       wrapped_vault_key = excluded.wrapped_vault_key,
                       org_id = excluded.org_id,
                       last_op_id_remote = MAX(last_op_id_remote, excluded.last_op_id_remote),
                       updated_at = excluded.updated_at"#,
                params![vault_id, name, role, wrapped, org_id, last_op_id_remote, now],
            )
        })
        .map_err(|e| ParaError::Db(format!("upsert sync_vault: {e}")))?;
    Ok(())
}

pub fn delete(store: &LocalStore, vault_id: &str) -> Result<()> {
    let vault_id = vault_id.to_string();
    store
        .with_conn(move |c| {
            c.execute("DELETE FROM sync_vaults WHERE vault_id = ?1", params![vault_id])
        })
        .map_err(|e| ParaError::Db(format!("delete sync_vault: {e}")))?;
    Ok(())
}

/// Unwrap the vault key for `vault_id` using the in-memory KEK. The KEK
/// is what unlocks the user's identity private key, which then opens
/// the sealed-box wrapping the vault key.
pub fn unwrap_vault_key(
    store: &LocalStore,
    vault_id: &str,
    kek: &KekMaterial,
) -> Result<VaultKey> {
    let wrapped: Option<Vec<u8>> = store
        .with_conn(|c| {
            c.query_row(
                "SELECT wrapped_vault_key FROM sync_vaults WHERE vault_id = ?1",
                params![vault_id],
                |r| r.get(0),
            )
            .optional()
        })
        .map_err(|e| ParaError::Db(format!("read wrapped vault key: {e}")))?;
    let wrapped = wrapped.ok_or_else(|| {
        ParaError::InvalidState(format!("vault {vault_id} not present locally"))
    })?;
    let (secret, _) = unwrap_identity_secret(store, kek)?;
    let pt = open_sealed(&secret, &wrapped)
        .map_err(|e| ParaError::Other(format!("vault key unwrap: {e}")))?;
    if pt.len() != 32 {
        return Err(ParaError::Other("vault key has wrong length".into()));
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&pt);
    Ok(VaultKey(k))
}
