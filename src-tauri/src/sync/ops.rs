//! Op codec + apply pipeline.
//!
//! Ops are the atomic unit of sync. Each op carries a `target_kind`
//! plus an opaque encrypted payload. The server never inspects the
//! payload — it just stores and broadcasts ops in id order.
//!
//! v1 supports:
//!
//! - `folder.upsert` — create or update a folder
//! - `folder.delete` — delete a folder (cascades to notes locally)
//! - `standalone_note.upsert` — create or update a standalone note
//! - `standalone_note.delete` — delete a standalone note
//!
//! Conflict policy: last-write-wins on `client_ts_ms`. If two devices
//! edit the same note offline and reconnect, the later write replaces
//! the earlier one. Per the cloud-architecture doc this is acceptable
//! for v1; CRDTs are explicitly deferred.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ParaError, Result};
use crate::store::db::LocalStore;
use crate::sync::crypto::{aead_open, aead_seal, VaultKey};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    FolderUpsert {
        remote_id: String,
        name: String,
        kind: String,
        color: Option<String>,
        description: Option<String>,
    },
    FolderDelete {
        remote_id: String,
    },
    StandaloneNoteUpsert {
        remote_id: String,
        folder_remote_id: Option<String>,
        title: String,
        text: String,
    },
    StandaloneNoteDelete {
        remote_id: String,
    },
}

impl Op {
    pub fn target_kind(&self) -> &'static str {
        match self {
            Op::FolderUpsert { .. } => "folder.upsert",
            Op::FolderDelete { .. } => "folder.delete",
            Op::StandaloneNoteUpsert { .. } => "standalone_note.upsert",
            Op::StandaloneNoteDelete { .. } => "standalone_note.delete",
        }
    }
}

/// Encrypt an op payload with the vault key. AAD pins the target_kind
/// so a server bug that swaps target_kind labels can't silently
/// reinterpret a folder op as a note op.
pub fn seal_op(op: &Op, vault_key: &VaultKey) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(op)
        .map_err(|e| ParaError::Other(format!("op encode: {e}")))?;
    aead_seal(&vault_key.0, &json, op.target_kind().as_bytes())
        .map_err(|e| ParaError::Other(format!("op seal: {e}")))
}

pub fn open_op(target_kind: &str, blob: &[u8], vault_key: &VaultKey) -> Result<Op> {
    let pt = aead_open(&vault_key.0, blob, target_kind.as_bytes())
        .map_err(|e| ParaError::Other(format!("op open: {e}")))?;
    serde_json::from_slice(&pt).map_err(|e| ParaError::Other(format!("op decode: {e}")))
}

/// Enqueue an op into the local outbox. The worker for this vault will
/// pick it up and Append it to the server.
pub fn enqueue(
    store: &LocalStore,
    vault_id: &str,
    op: &Op,
    vault_key: &VaultKey,
) -> Result<()> {
    let local_id = Uuid::new_v4().to_string();
    let target = op.target_kind().to_string();
    let payload = seal_op(op, vault_key)?;
    let now = chrono::Utc::now().timestamp_millis();
    let vault_id = vault_id.to_string();
    store
        .with_conn(move |c| {
            c.execute(
                r#"INSERT INTO sync_outbox
                   (local_id, vault_id, target_kind, payload_ciphertext, client_ts_ms, state, attempts, created_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?5)"#,
                params![local_id, vault_id, target, payload, now],
            )
        })
        .map_err(|e| ParaError::Db(format!("outbox insert: {e}")))?;
    Ok(())
}

/// One row in the outbox, returned by the worker when it dequeues
/// pending ops for a Append round.
#[derive(Debug)]
pub struct PendingOp {
    pub local_id: String,
    pub target_kind: String,
    pub payload: Vec<u8>,
    pub client_ts_ms: i64,
}

pub fn fetch_pending(store: &LocalStore, vault_id: &str, limit: usize) -> Result<Vec<PendingOp>> {
    let vault_id = vault_id.to_string();
    let limit = limit as i64;
    let rows: Vec<PendingOp> = store
        .with_conn(move |c| {
            let mut stmt = c.prepare(
                r#"SELECT local_id, target_kind, payload_ciphertext, client_ts_ms
                   FROM sync_outbox
                   WHERE vault_id = ?1 AND state = 'pending'
                   ORDER BY created_at ASC
                   LIMIT ?2"#,
            )?;
            let rows = stmt
                .query_map(params![vault_id, limit], |r| {
                    Ok(PendingOp {
                        local_id: r.get(0)?,
                        target_kind: r.get(1)?,
                        payload: r.get(2)?,
                        client_ts_ms: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .map_err(|e| ParaError::Db(format!("fetch pending: {e}")))?;
    Ok(rows)
}

pub fn mark_sent(store: &LocalStore, local_id: &str, op_id: i64) -> Result<()> {
    let local_id = local_id.to_string();
    store
        .with_conn(move |c| {
            c.execute(
                "UPDATE sync_outbox SET state = 'sent', last_error = NULL, attempts = attempts + 1 WHERE local_id = ?1",
                params![local_id],
            )?;
            // Remember the assigned id alongside the row so reconnects
            // don't risk re-sending. We just keep the row for the audit
            // trail; readers filter on state='pending'.
            c.execute(
                "UPDATE sync_outbox SET state = 'acked' WHERE local_id = ?1",
                params![local_id],
            )?;
            // Touch the corresponding entity if it cares about its op id.
            // standalone_notes.synced_op_id is set when the row is locally
            // applied; we don't need to chase that on the outbox path.
            let _ = op_id;
            Ok(())
        })
        .map_err(|e| ParaError::Db(format!("mark_sent: {e}")))?;
    Ok(())
}

/// Advance the cursor without applying an op. Used when an op is
/// undecryptable (e.g. its target_kind is from a future schema version)
/// so we don't get permanently stuck replaying it.
pub fn advance_cursor(store: &LocalStore, vault_id: &str, op_id: i64) -> Result<()> {
    let vault_id = vault_id.to_string();
    let now = chrono::Utc::now().timestamp();
    store
        .with_conn(move |c| {
            c.execute(
                r#"UPDATE sync_vaults
                       SET last_op_id_applied = MAX(last_op_id_applied, ?2),
                           last_op_id_remote  = MAX(last_op_id_remote,  ?2),
                           updated_at = ?3
                     WHERE vault_id = ?1"#,
                params![vault_id, op_id, now],
            )
        })
        .map_err(|e| ParaError::Db(format!("advance cursor: {e}")))?;
    Ok(())
}

pub fn mark_failed(store: &LocalStore, local_id: &str, err: &str) -> Result<()> {
    let local_id = local_id.to_string();
    let err = err.to_string();
    store
        .with_conn(move |c| {
            c.execute(
                "UPDATE sync_outbox SET attempts = attempts + 1, last_error = ?2 WHERE local_id = ?1",
                params![local_id, err],
            )
        })
        .map_err(|e| ParaError::Db(format!("mark_failed: {e}")))?;
    Ok(())
}

/// Apply a remote op to the local DB. Idempotent on (op_id) — calling
/// twice with the same op is a no-op because we update the cursor in
/// the same transaction.
pub fn apply_remote(
    store: &LocalStore,
    vault_id: &str,
    op_id: i64,
    op: &Op,
    client_ts_ms: i64,
) -> Result<()> {
    let vault_id = vault_id.to_string();
    let op = op.clone();
    let now = chrono::Utc::now().timestamp();
    store
        .with_conn(move |c| {
            // Skip if we've already seen this op.
            let last_applied: i64 = c
                .query_row(
                    "SELECT last_op_id_applied FROM sync_vaults WHERE vault_id = ?1",
                    params![vault_id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            if op_id <= last_applied {
                return Ok(());
            }

            match op {
                Op::FolderUpsert {
                    remote_id,
                    name,
                    kind,
                    color,
                    description,
                } => {
                    // Last-write-wins on client_ts_ms vs. local updated_at.
                    let existing: Option<(i64, i64)> = c
                        .query_row(
                            "SELECT id, updated_at FROM folders WHERE remote_id = ?1",
                            params![remote_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    if let Some((id, local_updated)) = existing {
                        if client_ts_ms / 1000 >= local_updated {
                            c.execute(
                                r#"UPDATE folders
                                       SET name = ?2, kind = ?3, color = ?4, description = ?5,
                                           updated_at = ?6, vault_id = ?7
                                     WHERE id = ?1"#,
                                params![id, name, kind, color, description, client_ts_ms / 1000, vault_id],
                            )?;
                        }
                    } else {
                        c.execute(
                            r#"INSERT INTO folders
                                   (name, kind, color, description, ownership_scope,
                                    owner_user_id, owner_org_id, vault_id, remote_id,
                                    created_at, updated_at)
                                   VALUES (?1, ?2, ?3, ?4, 'org_shared', NULL, NULL, ?5, ?6, ?7, ?7)"#,
                            params![name, kind, color, description, vault_id, remote_id, now],
                        )?;
                    }
                }
                Op::FolderDelete { remote_id } => {
                    c.execute(
                        "DELETE FROM folders WHERE remote_id = ?1 AND vault_id = ?2",
                        params![remote_id, vault_id],
                    )?;
                }
                Op::StandaloneNoteUpsert {
                    remote_id,
                    folder_remote_id,
                    title,
                    text,
                } => {
                    let folder_local: Option<i64> = match folder_remote_id.as_deref() {
                        Some(rid) => c
                            .query_row(
                                "SELECT id FROM folders WHERE remote_id = ?1",
                                params![rid],
                                |r| r.get(0),
                            )
                            .optional()?,
                        None => None,
                    };
                    let existing: Option<(i64, i64)> = c
                        .query_row(
                            "SELECT id, updated_at FROM standalone_notes WHERE remote_id = ?1",
                            params![remote_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    if let Some((id, local_updated)) = existing {
                        if client_ts_ms / 1000 >= local_updated {
                            c.execute(
                                r#"UPDATE standalone_notes
                                       SET title = ?2, text = ?3, folder_id = ?4,
                                           updated_at = ?5, vault_id = ?6, synced_op_id = ?7
                                     WHERE id = ?1"#,
                                params![
                                    id,
                                    title,
                                    text,
                                    folder_local,
                                    client_ts_ms / 1000,
                                    vault_id,
                                    op_id,
                                ],
                            )?;
                        }
                    } else {
                        c.execute(
                            r#"INSERT INTO standalone_notes
                                   (title, text, ownership_scope, folder_id, owner_user_id,
                                    owner_org_id, vault_id, remote_id, synced_op_id,
                                    created_at, updated_at)
                                   VALUES (?1, ?2, 'org_shared', ?3, NULL, NULL, ?4, ?5, ?6, ?7, ?7)"#,
                            params![
                                title,
                                text,
                                folder_local,
                                vault_id,
                                remote_id,
                                op_id,
                                client_ts_ms / 1000,
                            ],
                        )?;
                    }
                }
                Op::StandaloneNoteDelete { remote_id } => {
                    c.execute(
                        "DELETE FROM standalone_notes WHERE remote_id = ?1 AND vault_id = ?2",
                        params![remote_id, vault_id],
                    )?;
                }
            }

            // Advance the cursor. The row must already exist — it is
            // inserted at vault-discovery time by `register_vault` —
            // because we needed the vault key to decrypt this op.
            c.execute(
                r#"UPDATE sync_vaults
                       SET last_op_id_applied = MAX(last_op_id_applied, ?2),
                           last_op_id_remote  = MAX(last_op_id_remote,  ?2),
                           updated_at = ?3
                     WHERE vault_id = ?1"#,
                params![vault_id, op_id, now],
            )?;
            Ok(())
        })
        .map_err(|e: rusqlite::Error| ParaError::Db(format!("apply_remote: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::random_array;

    #[test]
    fn op_seal_round_trip() {
        let key = VaultKey(random_array::<32>().unwrap());
        let op = Op::StandaloneNoteUpsert {
            remote_id: "abc".into(),
            folder_remote_id: None,
            title: "hello".into(),
            text: "world".into(),
        };
        let blob = seal_op(&op, &key).unwrap();
        let decoded = open_op(op.target_kind(), &blob, &key).unwrap();
        match decoded {
            Op::StandaloneNoteUpsert { title, text, .. } => {
                assert_eq!(title, "hello");
                assert_eq!(text, "world");
            }
            _ => panic!("wrong variant"),
        }
    }
}
