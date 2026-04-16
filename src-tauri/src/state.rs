use std::sync::Mutex;

use crate::error::{ParaError, Result};
use crate::keyvault::vault::KeyVault;
use crate::store::db::LocalStore;

use tokio::runtime::Runtime;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub store: LocalStore,
    pub keyvault: KeyVault,
    pub rt: Runtime,
    /// One active capture session at a time (simplifies state).
    pub capture: Mutex<Option<CaptureHandle>>,
}

pub struct CaptureHandle {
    pub meeting_id: String,
    pub stop: tokio::sync::oneshot::Sender<()>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let keyvault = KeyVault::new("audire");

        // Try to get db encryption key from keyvault
        let db_key = keyvault.get_provider_key("dbkey");
        let store = LocalStore::open_default(db_key.as_deref())
            .map_err(|e| ParaError::Db(e.to_string()))?;

        // Tokio runtime tuned for desktop app (bounded memory, low overhead).
        // - worker_threads=2: sufficient for ASR websocket + audio pump
        // - thread_stack_size=512KiB: reduce idle RSS; no deep recursion needed
        // Reference: https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_stack_size(512 * 1024)
            .enable_all()
            .build()
            .map_err(|e| ParaError::Other(e.to_string()))?;

        Ok(Self {
            store,
            keyvault,
            rt,
            capture: Mutex::new(None),
        })
    }
}
