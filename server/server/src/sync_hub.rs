//! Per-process pub/sub for sync sockets.
//!
//! When a client appends an op via WebSocket, every other connected client
//! on the same vault should receive a `SyncMessage::Live` frame. For v1
//! we keep this in-process: a `tokio::sync::broadcast` channel per vault.
//! On a single Fly machine that's correct. When we scale beyond one
//! machine, we replace the inner with a Postgres LISTEN/NOTIFY-backed
//! broadcaster (Neon supports it on the same connection pool).

use std::collections::HashMap;
use std::sync::Arc;

use audire_shared::OpLogEntry;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Capacity of the per-vault broadcast channel. If a slow client falls
/// behind by more than this many ops, they get a `Lagged` error from
/// `recv()` and reconnect to do a fresh historical replay.
const PER_VAULT_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct SyncHub {
    inner: Arc<RwLock<HashMap<Uuid, broadcast::Sender<OpLogEntry>>>>,
}

impl SyncHub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe to live ops for a vault. Caller already holds the
    /// historical replay cursor; this stream covers everything that
    /// happens after subscribe-time.
    pub async fn subscribe(&self, vault_id: Uuid) -> broadcast::Receiver<OpLogEntry> {
        // Fast path: already a sender for this vault.
        {
            let read = self.inner.read().await;
            if let Some(tx) = read.get(&vault_id) {
                return tx.subscribe();
            }
        }
        // Slow path: create one. Re-check under the write lock to
        // avoid creating two senders for the same vault on a race.
        let mut write = self.inner.write().await;
        let entry = write
            .entry(vault_id)
            .or_insert_with(|| broadcast::channel(PER_VAULT_CAPACITY).0);
        entry.subscribe()
    }

    /// Publish a freshly-appended op to subscribers. Lossy on a slow
    /// receiver — that's fine, they reconnect and replay.
    pub async fn publish(&self, op: OpLogEntry) {
        let read = self.inner.read().await;
        if let Some(tx) = read.get(&op.vault_id) {
            // `send` returns Err(_) when there are no receivers; that's
            // expected and harmless.
            let _ = tx.send(op);
        }
    }
}

impl Default for SyncHub {
    fn default() -> Self {
        Self::new()
    }
}
