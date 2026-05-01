//! Audire shared types — used by both the Fly-hosted Sync server and the
//! Tauri desktop app. The contract here is the wire contract for Audire
//! Sync v1.
//!
//! The privacy invariant for everything in this crate: **the server cannot
//! decrypt any field whose name ends in `_ciphertext`, `wrapped_*`, or
//! `payload`.** Those bytes are produced and consumed by clients only.
//!
//! See `docs/cloud-architecture.md` in the audire repo for the full
//! specification this crate is the type-level shape of.

#![forbid(unsafe_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod sync;
pub use sync::*;

// ---- User / profile ----

/// A user as exposed by `GET /v1/users/me`. Email comes from Stack Auth's
/// claims and is plaintext on the server side; the public_key is X25519
/// raw bytes (32) used by other clients to wrap vault keys when sharing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: Uuid,
    pub email: String,
    #[serde(with = "serde_bytes_hex")]
    pub public_key: Vec<u8>,
    pub recovery_key_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Body of `POST /v1/users/me`. Sent by the client on first sign-in to
/// register their public key + recovery envelope. The server stores the
/// envelope and never sees the underlying recovery key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    #[serde(with = "serde_bytes_hex")]
    pub public_key: Vec<u8>,
    /// Master key wrapped with a recovery-key-derived KEK. Server stores
    /// this so users can recover access if they forget their passphrase
    /// but still have the recovery key.
    #[serde(with = "serde_bytes_hex")]
    pub wrapped_kek_for_recovery: Vec<u8>,
}

/// Response for `GET /v1/users/lookup?email=`. Returns the public key the
/// caller would need to wrap a vault key for the looked-up user. Only
/// resolves users who have already registered with Audire Sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLookupResponse {
    pub id: Uuid,
    pub email: String,
    #[serde(with = "serde_bytes_hex")]
    pub public_key: Vec<u8>,
}

// ---- Vault ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultRole {
    Owner,
    Editor,
    Reader,
}

/// One vault as visible to a single member. The server returns this shape
/// from `GET /v1/vaults` and `GET /v1/vaults/:id`. The `name_ciphertext`
/// is decrypted client-side using the unwrapped vault key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultView {
    pub id: Uuid,
    /// Vault name, encrypted with the vault key. Server cannot read it.
    #[serde(with = "serde_bytes_hex")]
    pub name_ciphertext: Vec<u8>,
    pub owner_user_id: Uuid,
    pub org_id: Option<Uuid>,
    /// The caller's copy of the vault key, wrapped to their KEK (for the
    /// owner) or sealed-box to their X25519 public key (for invited
    /// members).
    #[serde(with = "serde_bytes_hex")]
    pub wrapped_vault_key: Vec<u8>,
    pub role: VaultRole,
    pub last_op_id: i64,
    pub created_at: DateTime<Utc>,
}

/// Body of `POST /v1/vaults`. Server inserts the row and adds an `owner`
/// `vault_members` row in the same transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVaultRequest {
    /// Encrypted with the new vault key.
    #[serde(with = "serde_bytes_hex")]
    pub name_ciphertext: Vec<u8>,
    /// Vault key wrapped with the owner's KEK.
    #[serde(with = "serde_bytes_hex")]
    pub wrapped_vault_key: Vec<u8>,
    pub org_id: Option<Uuid>,
}

/// Body of `POST /v1/vaults/:id/members`. The caller has unwrapped the
/// vault key locally and re-wrapped it as a sealed box for the recipient's
/// public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMemberRequest {
    pub user_id: Uuid,
    #[serde(with = "serde_bytes_hex")]
    pub wrapped_vault_key: Vec<u8>,
    pub role: VaultRole,
}

/// Body of `POST /v1/vaults/:id/rotate-key`. Used after removing a
/// member: caller generates a new vault key, re-wraps for everyone who
/// remains, and uploads the bundle in one transactional call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateVaultKeyRequest {
    /// New encrypted name (re-encrypted with the new vault key).
    #[serde(with = "serde_bytes_hex")]
    pub name_ciphertext: Vec<u8>,
    /// One entry per remaining member. Must include the caller.
    pub wrapped_keys: Vec<RotatedMemberKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotatedMemberKey {
    pub user_id: Uuid,
    #[serde(with = "serde_bytes_hex")]
    pub wrapped_vault_key: Vec<u8>,
}

// ---- Organisations ----

/// Represents an organisation as visible to a single member. Orgs are an
/// optional scoping above vaults: a vault may belong to an org, in which
/// case org members can be invited as vault members without first having
/// to be looked up by email each time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgView {
    pub id: Uuid,
    pub name: String,
    pub owner_user_id: Uuid,
    pub role: OrgRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrgRole {
    Owner,
    Admin,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
}

/// Body of `POST /v1/orgs/:id/members`. The caller has resolved the
/// invitee via `/v1/users/lookup` and supplies their user_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddOrgMemberRequest {
    pub user_id: Uuid,
    pub role: OrgRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMemberView {
    pub user_id: Uuid,
    pub email: String,
    pub role: OrgRole,
}

// ---- Op log entries ----

/// A single op as stored by the server. Payload is opaque ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpLogEntry {
    pub id: i64,
    pub vault_id: Uuid,
    pub author_user_id: Uuid,
    pub device_id: Uuid,
    pub target_kind: String,
    #[serde(with = "serde_bytes_hex")]
    pub payload: Vec<u8>,
    pub client_ts_ms: i64,
    pub created_at: DateTime<Utc>,
}

/// What a client sends when appending. The server assigns the `id` and
/// `created_at` and echoes back via `SyncMessage::Ack` then `Live`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOp {
    /// Client-assigned UUID so the originating device can correlate the
    /// `Ack` back to the right local pending op.
    pub local_id: Uuid,
    pub device_id: Uuid,
    pub target_kind: String,
    #[serde(with = "serde_bytes_hex")]
    pub payload: Vec<u8>,
    pub client_ts_ms: i64,
}

// ---- Errors that come back as JSON ----

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

// ---- Hex byte serde helper ----

/// Serialize/deserialize `Vec<u8>` as a lowercase hex string in JSON. We
/// use hex rather than base64 so the wire format is grep-friendly when
/// debugging; the volume is dominated by op payloads (encrypted blobs)
/// and the 2x size overhead vs. base64 is acceptable for v1.
mod serde_bytes_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let hex = bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        s.serialize_str(&hex)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() % 2 != 0 {
            return Err(serde::de::Error::custom("hex string has odd length"));
        }
        let mut out = Vec::with_capacity(s.len() / 2);
        for i in (0..s.len()).step_by(2) {
            let byte = u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(serde::de::Error::custom)?;
            out.push(byte);
        }
        Ok(out)
    }
}
