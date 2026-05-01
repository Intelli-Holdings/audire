//! Optional cloud sync for Audire.
//!
//! # Privacy invariants
//!
//! - **Audio is never uploaded.** Nothing in this module reads from
//!   `audio/` or touches PCM buffers. The schema-level guarantee in
//!   `store::db` already prevents audio from being persisted; this
//!   module only ever sends *encrypted note/folder ops*.
//! - **Keys never leave the device in plaintext.** The user's KEK is
//!   derived locally from a passphrase via Argon2id and held only in
//!   RAM. Vault keys are wrapped per-recipient using X25519 + XChaCha20.
//!   The server stores only opaque ciphertext.
//! - **Sync is opt-in.** Without an `Account` row in the local DB, no
//!   network traffic is generated.

pub mod account;
pub mod api;
pub mod client;
pub mod crypto;
pub mod manager;
pub mod ops;
pub mod orgs;
pub mod vaults;
pub mod worker;

pub use account::{AccountStatus, SignInRequest, SignUpRequest};
pub use client::SyncClient;
pub use crypto::{KekMaterial, VaultKey};
