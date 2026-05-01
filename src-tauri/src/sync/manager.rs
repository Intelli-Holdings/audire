//! In-process owner of the per-vault sync workers + the in-memory KEK.
//!
//! Locked the moment the user signs out; cleared on app exit.
//!
//! The manager is held in `AppState.sync_runtime`. IPC commands that
//! need a vault key (create_org, invite, enqueue ops) borrow the KEK
//! from here.

use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use crate::error::{ParaError, Result};
use crate::store::db::LocalStore;
use crate::sync::crypto::KekMaterial;
use crate::sync::vaults as local_vaults;
use crate::sync::worker::{self, WorkerConfig, WorkerHandle};

pub struct SyncRuntime {
    inner: Mutex<Inner>,
}

struct Inner {
    /// User's KEK while the session is unlocked. `None` means the user
    /// must re-enter their passphrase before any vault op runs.
    kek: Option<KekMaterial>,
    workers: HashMap<String, WorkerHandle>,
    device_id: Uuid,
}

impl SyncRuntime {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                kek: None,
                workers: HashMap::new(),
                device_id: Uuid::new_v4(),
            }),
        }
    }

    pub fn unlock(&self, kek: KekMaterial) {
        let mut g = self.inner.lock().expect("sync runtime poisoned");
        g.kek = Some(kek);
    }

    pub fn is_unlocked(&self) -> bool {
        self.inner
            .lock()
            .map(|g| g.kek.is_some())
            .unwrap_or(false)
    }

    pub fn with_kek<R>(&self, f: impl FnOnce(&KekMaterial) -> Result<R>) -> Result<R> {
        let g = self.inner.lock().expect("sync runtime poisoned");
        let kek = g
            .kek
            .as_ref()
            .ok_or_else(|| ParaError::InvalidState("sync is locked — unlock with passphrase".into()))?;
        f(kek)
    }

    pub fn device_id(&self) -> Uuid {
        self.inner
            .lock()
            .map(|g| g.device_id)
            .unwrap_or_else(|_| Uuid::new_v4())
    }

    /// Stop every worker and forget the KEK. Idempotent.
    pub fn shutdown(&self) {
        let mut g = self.inner.lock().expect("sync runtime poisoned");
        for (_id, h) in g.workers.drain() {
            let _ = h.stop.send(());
        }
        g.kek = None;
    }

    /// Start (or restart) workers for every vault present in
    /// `sync_vaults`. Workers that are already running are left alone.
    pub fn start_workers(
        &self,
        rt: &tokio::runtime::Runtime,
        app: &tauri::AppHandle,
        store: &LocalStore,
        server_url: &str,
        access_token: &str,
    ) -> Result<()> {
        let kek_clone = {
            let g = self.inner.lock().expect("sync runtime poisoned");
            g.kek
                .as_ref()
                .ok_or_else(|| {
                    ParaError::InvalidState("cannot start workers while locked".into())
                })?
                .clone()
        };
        let device_id = self.device_id();
        let vaults = local_vaults::list(store)?;
        let mut g = self.inner.lock().expect("sync runtime poisoned");
        for v in vaults {
            if g.workers.contains_key(&v.vault_id) {
                continue;
            }
            // Unwrap the vault key once and hand it to the worker.
            let vault_key = match local_vaults::unwrap_vault_key(store, &v.vault_id, &kek_clone) {
                Ok(k) => k,
                Err(e) => {
                    tracing::warn!(
                        target: "audire-sync",
                        vault = %v.vault_id,
                        "could not unwrap vault key — skipping worker: {e}"
                    );
                    continue;
                }
            };
            let cfg = WorkerConfig {
                server_url: server_url.to_string(),
                access_token: access_token.to_string(),
                vault_id: v.vault_id.clone(),
                device_id,
            };
            let handle = worker::spawn(rt, app.clone(), store.clone(), cfg, vault_key);
            g.workers.insert(v.vault_id.clone(), handle);
        }
        Ok(())
    }

    pub fn stop_worker(&self, vault_id: &str) {
        let mut g = self.inner.lock().expect("sync runtime poisoned");
        if let Some(h) = g.workers.remove(vault_id) {
            let _ = h.stop.send(());
        }
    }

    pub fn running_vaults(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|g| g.workers.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for SyncRuntime {
    fn default() -> Self {
        Self::new()
    }
}
