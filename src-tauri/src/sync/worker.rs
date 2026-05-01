//! Per-vault sync worker.
//!
//! One tokio task per vault. Lifecycle:
//!
//! 1. Connect WebSocket to `/v1/sync/:vault_id?since=<cursor>`.
//! 2. Read frames; for `Ops` and `Live`, decrypt + apply via `ops::apply_remote`.
//! 3. Drain `sync_outbox` by sending `Append` frames; await `Ack` and mark
//!    the row `acked`.
//! 4. On any error or close, emit `Disconnected`, sleep with exponential
//!    backoff (capped at 30s), and reconnect.
//!
//! The worker holds an in-memory copy of the vault key, derived once at
//! start-up via the KEK supplied by the Account UI's "unlock" step.
//! The KEK itself is *not* persisted; if the app restarts, the user is
//! re-prompted (or a future `passphrase-cached` toggle will keep it in
//! the OS keyring).

use std::time::Duration;

use anyhow::{anyhow, Context};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{HeaderValue, AUTHORIZATION};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::store::db::LocalStore;
use crate::sync::crypto::VaultKey;
use crate::sync::ops;

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatusEvent {
    pub vault_id: String,
    pub state: String,
    pub message: Option<String>,
    pub last_op_id_applied: i64,
}

pub struct WorkerHandle {
    pub vault_id: String,
    pub stop: oneshot::Sender<()>,
}

#[derive(Clone)]
pub struct WorkerConfig {
    pub server_url: String,
    pub access_token: String,
    pub vault_id: String,
    pub device_id: Uuid,
}

/// Spawn a worker that runs until the stop oneshot fires.
pub fn spawn(
    rt: &tokio::runtime::Runtime,
    app: AppHandle,
    store: LocalStore,
    cfg: WorkerConfig,
    vault_key: VaultKey,
) -> WorkerHandle {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let vault_id_for_handle = cfg.vault_id.clone();
    rt.spawn(async move {
        run(app, store, cfg, vault_key, stop_rx).await;
    });
    WorkerHandle {
        vault_id: vault_id_for_handle,
        stop: stop_tx,
    }
}

async fn run(
    app: AppHandle,
    store: LocalStore,
    cfg: WorkerConfig,
    vault_key: VaultKey,
    mut stop_rx: oneshot::Receiver<()>,
) {
    let mut backoff_secs = 1u64;
    loop {
        let cursor = read_cursor(&store, &cfg.vault_id).unwrap_or(0);

        emit_status(
            &app,
            &cfg.vault_id,
            "connecting",
            None,
            cursor,
        );

        match connect_and_run(&app, &store, &cfg, &vault_key, cursor, &mut stop_rx).await {
            Ok(StopReason::Stopped) => {
                emit_status(&app, &cfg.vault_id, "stopped", None, cursor);
                return;
            }
            Ok(StopReason::Disconnected(reason)) => {
                emit_status(
                    &app,
                    &cfg.vault_id,
                    "disconnected",
                    Some(reason),
                    cursor,
                );
            }
            Err(e) => {
                emit_status(
                    &app,
                    &cfg.vault_id,
                    "error",
                    Some(format!("{e:#}")),
                    cursor,
                );
            }
        }

        // Backoff before reconnect; tokio::select with stop so a stop
        // request during sleep wakes us up immediately.
        let sleep = tokio::time::sleep(Duration::from_secs(backoff_secs));
        tokio::pin!(sleep);
        tokio::select! {
            _ = &mut sleep => {}
            _ = &mut stop_rx => {
                emit_status(&app, &cfg.vault_id, "stopped", None, cursor);
                return;
            }
        }
        backoff_secs = (backoff_secs.saturating_mul(2)).min(30);
    }
}

enum StopReason {
    Stopped,
    Disconnected(String),
}

async fn connect_and_run(
    app: &AppHandle,
    store: &LocalStore,
    cfg: &WorkerConfig,
    vault_key: &VaultKey,
    since: i64,
    stop_rx: &mut oneshot::Receiver<()>,
) -> anyhow::Result<StopReason> {
    let url = ws_url(&cfg.server_url, &cfg.vault_id, since);
    let mut req = url.as_str().into_client_request().context("ws request")?;
    req.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", cfg.access_token))
            .map_err(|_| anyhow!("invalid bearer token"))?,
    );

    let (ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .context("ws connect")?;
    let (mut sink, mut stream) = ws.split();

    let mut last_applied = since;

    // Drain outbox in a loop alongside reading from the socket. We use
    // a tokio::interval rather than draining only after CaughtUp so a
    // burst of outbound writes can flow even before replay finishes.
    let mut drain_tick = tokio::time::interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            _ = &mut *stop_rx => {
                let _ = sink.send(Message::Close(None)).await;
                return Ok(StopReason::Stopped);
            }
            msg = stream.next() => {
                let Some(msg) = msg else {
                    return Ok(StopReason::Disconnected("server closed stream".into()));
                };
                let msg = msg.context("ws read")?;
                match msg {
                    Message::Text(s) => {
                        if let Some(reason) = handle_text(&app, &store, cfg, vault_key, &s, &mut last_applied)
                            .await
                            .context("handle text frame")?
                        {
                            return Ok(StopReason::Disconnected(reason));
                        }
                    }
                    Message::Binary(b) => {
                        let s = String::from_utf8_lossy(&b).to_string();
                        if let Some(reason) = handle_text(&app, &store, cfg, vault_key, &s, &mut last_applied)
                            .await
                            .context("handle binary frame")?
                        {
                            return Ok(StopReason::Disconnected(reason));
                        }
                    }
                    Message::Ping(p) => { let _ = sink.send(Message::Pong(p)).await; }
                    Message::Pong(_) => {}
                    Message::Close(_) => {
                        return Ok(StopReason::Disconnected("server closed".into()));
                    }
                    Message::Frame(_) => {}
                }
            }
            _ = drain_tick.tick() => {
                drain_outbox(&store, cfg, vault_key, &mut sink).await?;
            }
        }
    }
}

async fn handle_text(
    app: &AppHandle,
    store: &LocalStore,
    cfg: &WorkerConfig,
    vault_key: &VaultKey,
    s: &str,
    last_applied: &mut i64,
) -> anyhow::Result<Option<String>> {
    let frame: ServerFrame =
        serde_json::from_str(s).with_context(|| format!("decode frame: {s}"))?;
    match frame {
        ServerFrame::CaughtUp { last_op_id, .. } => {
            emit_status(app, &cfg.vault_id, "live", None, *last_applied);
            // CaughtUp may report ops past our cursor that we skipped
            // because they originated here. Persist the high-water
            // mark for the next reconnect's cursor.
            persist_remote_high_water(store, &cfg.vault_id, last_op_id);
        }
        ServerFrame::Ops { ops: ops_vec, .. } => {
            for op in ops_vec {
                apply_one(store, cfg, vault_key, op, last_applied)?;
            }
            emit_status(app, &cfg.vault_id, "syncing", None, *last_applied);
        }
        ServerFrame::Live { op, .. } => {
            apply_one(store, cfg, vault_key, op, last_applied)?;
            emit_status(app, &cfg.vault_id, "live", None, *last_applied);
        }
        ServerFrame::Ack {
            local_id, op_id, ..
        } => {
            ops::mark_sent(store, &local_id.to_string(), op_id)
                .map_err(|e| anyhow!("mark_sent: {e}"))?;
        }
        ServerFrame::Error { code, message } => {
            return Ok(Some(format!("server: {code}: {message}")));
        }
        ServerFrame::Ping => { /* no-op; tokio_tungstenite auto-replies via Ping/Pong on transport */ }
        ServerFrame::Pong => {}
    }
    Ok(None)
}

fn apply_one(
    store: &LocalStore,
    cfg: &WorkerConfig,
    vault_key: &VaultKey,
    op: ServerOp,
    last_applied: &mut i64,
) -> anyhow::Result<()> {
    let payload = hex::decode(&op.payload).context("hex payload")?;
    match ops::open_op(&op.target_kind, &payload, vault_key) {
        Ok(parsed) => {
            ops::apply_remote(store, &cfg.vault_id, op.id, &parsed, op.client_ts_ms)
                .map_err(|e| anyhow!("apply: {e}"))?;
            *last_applied = op.id.max(*last_applied);
        }
        Err(e) => {
            // Don't fail the whole stream on a single bad op — log and
            // skip. Cursor still advances so we don't get stuck.
            tracing::warn!(
                target: "audire-sync",
                op_id = op.id,
                "skipping undecryptable op: {e}"
            );
            ops::advance_cursor(store, &cfg.vault_id, op.id)
                .map_err(|e| anyhow!("advance_cursor: {e}"))?;
            *last_applied = op.id.max(*last_applied);
        }
    }
    Ok(())
}

async fn drain_outbox<S>(
    store: &LocalStore,
    cfg: &WorkerConfig,
    _vault_key: &VaultKey,
    sink: &mut S,
) -> anyhow::Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let pending = ops::fetch_pending(store, &cfg.vault_id, 32)
        .map_err(|e| anyhow!("fetch_pending: {e}"))?;
    for p in pending {
        let frame = serde_json::json!({
            "type": "append",
            "vault_id": cfg.vault_id,
            "op": {
                "local_id": p.local_id,
                "device_id": cfg.device_id,
                "target_kind": p.target_kind,
                "payload": hex::encode(&p.payload),
                "client_ts_ms": p.client_ts_ms,
            },
        });
        sink.send(Message::Text(frame.to_string()))
            .await
            .context("send append")?;
    }
    Ok(())
}

fn read_cursor(store: &LocalStore, vault_id: &str) -> Option<i64> {
    store
        .with_conn(|c| {
            c.query_row(
                "SELECT last_op_id_applied FROM sync_vaults WHERE vault_id = ?1",
                rusqlite::params![vault_id],
                |r| r.get::<_, i64>(0),
            )
            .map(Some)
            .or_else(|_| Ok::<_, rusqlite::Error>(None))
        })
        .ok()
        .flatten()
}

fn persist_remote_high_water(store: &LocalStore, vault_id: &str, op_id: i64) {
    let _ = store.with_conn(|c| {
        c.execute(
            r#"UPDATE sync_vaults
                   SET last_op_id_remote = MAX(last_op_id_remote, ?2)
                 WHERE vault_id = ?1"#,
            rusqlite::params![vault_id, op_id],
        )
    });
}

fn ws_url(base: &str, vault_id: &str, since: i64) -> String {
    let scheme = base
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let trimmed = scheme.trim_end_matches('/');
    format!("{trimmed}/v1/sync/{vault_id}?since={since}")
}

fn emit_status(
    app: &AppHandle,
    vault_id: &str,
    state: &str,
    message: Option<String>,
    last_op_id_applied: i64,
) {
    let _ = app.emit(
        "audire://sync_status",
        SyncStatusEvent {
            vault_id: vault_id.to_string(),
            state: state.to_string(),
            message,
            last_op_id_applied,
        },
    );
}

// Subset of the server's `SyncMessage` enum we currently parse. Other
// variants are dropped so a server upgrade doesn't crash old clients.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerFrame {
    Ops {
        #[allow(dead_code)]
        vault_id: String,
        ops: Vec<ServerOp>,
    },
    CaughtUp {
        #[allow(dead_code)]
        vault_id: String,
        last_op_id: i64,
    },
    Live {
        #[allow(dead_code)]
        vault_id: String,
        op: ServerOp,
    },
    Ack {
        local_id: Uuid,
        op_id: i64,
        #[allow(dead_code)]
        client_ts_ms: i64,
    },
    Error {
        code: String,
        message: String,
    },
    Ping,
    Pong,
}

#[derive(Deserialize)]
struct ServerOp {
    id: i64,
    #[allow(dead_code)]
    vault_id: String,
    #[allow(dead_code)]
    author_user_id: String,
    #[allow(dead_code)]
    device_id: String,
    target_kind: String,
    /// Hex-encoded ciphertext, per `audire_shared::OpLogEntry`'s
    /// `serde_bytes_hex` representation.
    payload: String,
    client_ts_ms: i64,
}
