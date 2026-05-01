//! High-level org/vault flows used by the IPC layer.
//!
//! The desktop hides the vault concept from end-users. The user-facing
//! noun is "organisation"; under the hood every org has exactly one
//! default vault that all org members are also members of.
//!
//! These functions need an unlocked KEK to wrap/unwrap vault keys. The
//! IPC layer is responsible for prompting the user for their
//! passphrase if the KEK isn't in `SyncRuntime`.

use anyhow::{anyhow, Context};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::error::{ParaError, Result};
use crate::store::db::LocalStore;
use crate::sync::account::unwrap_identity_secret;
use crate::sync::client::SyncClient;
use crate::sync::crypto::{aead_seal, new_vault_key, seal_for_recipient, KekMaterial, VaultKey};
use crate::sync::orgs;
use crate::sync::vaults as local_vaults;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrgArgs {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateOrgOutcome {
    pub org_id: String,
    pub default_vault_id: String,
}

/// Create a new org with a default vault. Returns ids the UI can show.
pub async fn create_org(
    store: &LocalStore,
    server_url: &str,
    access_token: &str,
    kek: &KekMaterial,
    args: &CreateOrgArgs,
) -> Result<CreateOrgOutcome> {
    let name = args.name.trim();
    if name.is_empty() {
        return Err(ParaError::InvalidState("org name cannot be empty".into()));
    }
    let client = SyncClient::new(server_url, access_token);

    let org = client
        .create_org(name)
        .await
        .map_err(|e| ParaError::Other(format!("create_org failed: {e}")))?;

    // Default vault for the org. Encrypt the org name as the vault
    // name so the server only ever sees ciphertext for vault metadata.
    let vault_key = new_vault_key().map_err(|e| ParaError::Other(e.to_string()))?;
    let name_ct = aead_seal(&vault_key.0, name.as_bytes(), b"audire-vault-name v1")
        .map_err(|e| ParaError::Other(format!("vault name encrypt: {e}")))?;

    // Wrap the vault key for the caller. We use the same sealed-box
    // construction we use for other recipients so the unwrap path on
    // the local device is uniform.
    let (_, my_pub) = unwrap_identity_secret(store, kek)?;
    let wrapped_self = seal_for_recipient(&my_pub, &vault_key.0)
        .map_err(|e| ParaError::Other(format!("self-wrap vault key: {e}")))?;

    let vault = client
        .create_vault(&name_ct, &wrapped_self, Some(&org.id))
        .await
        .map_err(|e| ParaError::Other(format!("create_vault failed: {e}")))?;

    // Persist locally so the worker can find it.
    orgs::upsert(store, &org.id, &org.name, &org.role, Some(&org.owner_user_id))?;
    local_vaults::upsert(
        store,
        &vault.id,
        name,
        &vault.role,
        Some(&org.id),
        &wrapped_self,
        vault.last_op_id,
    )?;

    Ok(CreateOrgOutcome {
        org_id: org.id,
        default_vault_id: vault.id,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct InviteToOrgArgs {
    pub org_id: String,
    pub email: String,
    /// "owner" / "admin" / "member" (org role) and "owner" / "editor" /
    /// "reader" (vault role). Default = "member" + "editor".
    pub org_role: Option<String>,
    pub vault_role: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InviteToOrgOutcome {
    pub user_id: String,
    pub email: String,
}

/// Invite `email` to `org_id`. Looks up the user, adds them to the
/// org, then re-wraps every vault key in the org for their public key
/// and adds them as a vault member. Errors if the invitee hasn't yet
/// signed up to Audire Sync.
pub async fn invite_to_org(
    store: &LocalStore,
    server_url: &str,
    access_token: &str,
    kek: &KekMaterial,
    args: &InviteToOrgArgs,
) -> Result<InviteToOrgOutcome> {
    let email = args.email.trim();
    if email.is_empty() {
        return Err(ParaError::InvalidState("email cannot be empty".into()));
    }
    let client = SyncClient::new(server_url, access_token);

    let invitee = client
        .lookup_user(email)
        .await
        .map_err(|e| ParaError::Other(format!("lookup failed: {e}")))?;
    let invitee_pub_bytes = hex::decode(&invitee.public_key)
        .map_err(|e| ParaError::Other(format!("public_key decode: {e}")))?;
    if invitee_pub_bytes.len() != 32 {
        return Err(ParaError::Other("invitee public_key wrong length".into()));
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&invitee_pub_bytes);
    let invitee_pub = x25519_dalek::PublicKey::from(pk);

    let org_role = args.org_role.as_deref().unwrap_or("member");
    let vault_role = args.vault_role.as_deref().unwrap_or("editor");

    client
        .add_org_member(&args.org_id, &invitee.id, org_role)
        .await
        .map_err(|e| ParaError::Other(format!("add_org_member: {e}")))?;

    // Re-wrap every local vault that belongs to this org for the
    // invitee. We unwrap the vault key with the local KEK, then sealed
    // -box it for their public key.
    let vaults = local_vaults::list(store)?;
    for v in vaults.iter().filter(|v| v.org_id.as_deref() == Some(args.org_id.as_str())) {
        let vault_key: VaultKey =
            local_vaults::unwrap_vault_key(store, &v.vault_id, kek)?;
        let wrapped_for_them = seal_for_recipient(&invitee_pub, &vault_key.0)
            .map_err(|e| ParaError::Other(format!("seal vault key: {e}")))?;
        client
            .add_vault_member(&v.vault_id, &invitee.id, &wrapped_for_them, vault_role)
            .await
            .map_err(|e| ParaError::Other(format!("add_vault_member: {e}")))?;
    }

    Ok(InviteToOrgOutcome {
        user_id: invitee.id,
        email: invitee.email,
    })
}

/// Pull the latest list of vaults from the server and reconcile the
/// local cache. Called on sign-in and any time the user clicks
/// "Refresh" in the Account UI.
pub async fn refresh_vaults(
    store: &LocalStore,
    server_url: &str,
    access_token: &str,
) -> Result<Vec<local_vaults::LocalVaultRow>> {
    let client = SyncClient::new(server_url, access_token);
    let remote = client
        .list_vaults()
        .await
        .map_err(|e| ParaError::Other(format!("list_vaults: {e}")))?;
    let orgs_remote = client
        .list_orgs()
        .await
        .map_err(|e| ParaError::Other(format!("list_orgs: {e}")))?;
    for o in &orgs_remote {
        orgs::upsert(store, &o.id, &o.name, &o.role, Some(&o.owner_user_id))?;
    }
    for v in &remote {
        let wrapped = hex::decode(&v.wrapped_vault_key)
            .map_err(|e| ParaError::Other(format!("wrapped_vault_key decode: {e}")))?;
        // We don't try to decrypt the name here — that needs a KEK we
        // may not currently hold in memory. The local row's `name`
        // field is best-effort and refreshed when the user creates the
        // vault locally; for invites it stays empty until they unlock.
        local_vaults::upsert(
            store,
            &v.id,
            "",
            &v.role,
            v.org_id.as_deref(),
            &wrapped,
            v.last_op_id,
        )?;
    }
    local_vaults::list(store)
}

/// Look up the local org row backing a vault, used by the sidebar's
/// "share with org" picker.
pub fn vault_org_for(store: &LocalStore, vault_id: &str) -> Result<Option<String>> {
    let vault_id = vault_id.to_string();
    let row: Option<String> = store
        .with_conn(|c| {
            c.query_row(
                "SELECT org_id FROM sync_vaults WHERE vault_id = ?1",
                rusqlite::params![vault_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
                .map(|opt| opt.flatten())
        })
        .map_err(|e| ParaError::Db(format!("vault_org_for: {e}")))?;
    Ok(row)
}

/// Lookup the vault id + remote_id for a folder; if the folder isn't
/// bound to a vault this returns `Ok(None)` and the caller skips
/// enqueueing.
pub fn folder_sync_target(
    store: &LocalStore,
    folder_id: i64,
) -> Result<Option<(String, String)>> {
    let row: Option<(Option<String>, Option<String>)> = store
        .with_conn(|c| {
            c.query_row(
                "SELECT vault_id, remote_id FROM folders WHERE id = ?1",
                rusqlite::params![folder_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
        .map_err(|e| ParaError::Db(format!("folder_sync_target: {e}")))?;
    Ok(match row {
        Some((Some(vault), Some(remote))) => Some((vault, remote)),
        _ => None,
    })
}

pub fn note_sync_target(
    store: &LocalStore,
    note_id: i64,
) -> Result<Option<(String, String, Option<String>)>> {
    // Returns (vault_id, note_remote_id, folder_remote_id)
    let row: Option<(Option<String>, Option<String>, Option<i64>)> = store
        .with_conn(|c| {
            c.query_row(
                "SELECT vault_id, remote_id, folder_id FROM standalone_notes WHERE id = ?1",
                rusqlite::params![note_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
        })
        .map_err(|e| ParaError::Db(format!("note_sync_target: {e}")))?;
    Ok(match row {
        Some((Some(vault), Some(remote), folder_id)) => {
            let folder_remote = match folder_id {
                Some(fid) => store
                    .with_conn(|c| {
                        c.query_row(
                            "SELECT remote_id FROM folders WHERE id = ?1",
                            rusqlite::params![fid],
                            |r| r.get::<_, Option<String>>(0),
                        )
                        .optional()
                    })
                    .map_err(|e| ParaError::Db(format!("note folder_remote: {e}")))?
                    .flatten(),
                None => None,
            };
            Some((vault, remote, folder_remote))
        }
        _ => None,
    })
}

/// Bind a local folder to a vault. Generates a remote_id, persists the
/// link, and enqueues a `folder.upsert` op so other org members see
/// the folder.
pub fn share_folder_with_org(
    store: &LocalStore,
    folder_id: i64,
    org_id: &str,
    kek: &KekMaterial,
) -> Result<String> {
    // Find the org's default vault (any vault belonging to that org;
    // v1 has at most one).
    let vaults = local_vaults::list(store)?;
    let v = vaults
        .iter()
        .find(|v| v.org_id.as_deref() == Some(org_id))
        .ok_or_else(|| {
            ParaError::InvalidState(format!("no vault found for org {org_id}"))
        })?;
    let vault_key = local_vaults::unwrap_vault_key(store, &v.vault_id, kek)?;

    let folder = store
        .with_conn(|c| {
            c.query_row(
                "SELECT name, kind, color, description, remote_id FROM folders WHERE id = ?1",
                rusqlite::params![folder_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                },
            )
        })
        .map_err(|e| ParaError::Db(format!("read folder: {e}")))?;

    let (name, kind, color, description, existing_remote) = folder;
    let remote_id = existing_remote.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let vid = v.vault_id.clone();
    let remote_for_db = remote_id.clone();
    store
        .with_conn(move |c| {
            c.execute(
                "UPDATE folders SET vault_id = ?2, remote_id = ?3, ownership_scope = 'org_shared' WHERE id = ?1",
                rusqlite::params![folder_id, vid, remote_for_db],
            )
        })
        .map_err(|e| ParaError::Db(format!("bind folder: {e}")))?;

    let op = crate::sync::ops::Op::FolderUpsert {
        remote_id: remote_id.clone(),
        name,
        kind,
        color,
        description,
    };
    crate::sync::ops::enqueue(store, &v.vault_id, &op, &vault_key)?;
    Ok(remote_id)
}

/// Convenience helper used by the existing `update_standalone_note`
/// IPC handler. Looks up the binding and, if present, enqueues an op.
/// No-op if the note isn't bound to a vault.
pub fn enqueue_note_upsert(
    store: &LocalStore,
    note_id: i64,
    title: &str,
    text: &str,
    kek: &KekMaterial,
) -> Result<()> {
    let target = match note_sync_target(store, note_id)? {
        Some(t) => t,
        None => return Ok(()),
    };
    let (vault_id, note_remote, folder_remote) = target;
    let vault_key = local_vaults::unwrap_vault_key(store, &vault_id, kek)?;
    let op = crate::sync::ops::Op::StandaloneNoteUpsert {
        remote_id: note_remote,
        folder_remote_id: folder_remote,
        title: title.to_string(),
        text: text.to_string(),
    };
    crate::sync::ops::enqueue(store, &vault_id, &op, &vault_key)
}

pub fn enqueue_note_delete(store: &LocalStore, note_id: i64, kek: &KekMaterial) -> Result<()> {
    let target = match note_sync_target(store, note_id)? {
        Some(t) => t,
        None => return Ok(()),
    };
    let (vault_id, note_remote, _) = target;
    let vault_key = local_vaults::unwrap_vault_key(store, &vault_id, kek)?;
    let op = crate::sync::ops::Op::StandaloneNoteDelete { remote_id: note_remote };
    crate::sync::ops::enqueue(store, &vault_id, &op, &vault_key)
}

pub fn enqueue_folder_upsert(
    store: &LocalStore,
    folder_id: i64,
    kek: &KekMaterial,
) -> Result<()> {
    let target = match folder_sync_target(store, folder_id)? {
        Some(t) => t,
        None => return Ok(()),
    };
    let (vault_id, folder_remote) = target;
    let vault_key = local_vaults::unwrap_vault_key(store, &vault_id, kek)?;
    let folder = store
        .with_conn(|c| {
            c.query_row(
                "SELECT name, kind, color, description FROM folders WHERE id = ?1",
                rusqlite::params![folder_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
        })
        .map_err(|e| ParaError::Db(format!("read folder for op: {e}")))?;
    let op = crate::sync::ops::Op::FolderUpsert {
        remote_id: folder_remote,
        name: folder.0,
        kind: folder.1,
        color: folder.2,
        description: folder.3,
    };
    crate::sync::ops::enqueue(store, &vault_id, &op, &vault_key)
}

pub fn enqueue_folder_delete(
    store: &LocalStore,
    folder_id: i64,
    kek: &KekMaterial,
) -> Result<()> {
    let target = match folder_sync_target(store, folder_id)? {
        Some(t) => t,
        None => return Ok(()),
    };
    let (vault_id, folder_remote) = target;
    let vault_key = local_vaults::unwrap_vault_key(store, &vault_id, kek)?;
    let op = crate::sync::ops::Op::FolderDelete { remote_id: folder_remote };
    crate::sync::ops::enqueue(store, &vault_id, &op, &vault_key)
}
