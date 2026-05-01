//! `WS /v1/sync/:vault_id` — the actual sync stream.
//!
//! On connect:
//! 1. Verify the caller has a `vault_members` row for the vault.
//! 2. Replay every op > the `?since=` cursor in chunks of 256.
//! 3. Send a `CaughtUp` frame.
//! 4. Then keep both directions: incoming `Append` from this client gets
//!    inserted into `op_log` and broadcast via `SyncHub`; ops that other
//!    clients have appended come back through the broadcast channel.

use audire_shared::{NewOp, OpLogEntry, SyncMessage};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const REPLAY_CHUNK: i64 = 256;

#[derive(Debug, Default, Deserialize)]
pub struct SyncQuery {
    /// Cursor: only ops with `id > since` are replayed. Defaults to 0 =
    /// full history. Clients pass their last-acked op id here so a
    /// reconnect doesn't replay everything.
    #[serde(default)]
    pub since: i64,
}

pub async fn websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(vault_id): Path<Uuid>,
    Query(q): Query<SyncQuery>,
) -> ApiResult<impl IntoResponse> {
    // Authorisation check happens before the upgrade so unauthorised
    // callers get a clean 403 rather than a closed socket.
    let role = sqlx::query_scalar!(
        r#"SELECT role FROM audire.vault_members WHERE vault_id = $1 AND user_id = $2"#,
        vault_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::NotFound("vault not found or no access".into()))?;

    let writable = role == "owner" || role == "editor";

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(e) = run_socket(socket, state, auth, vault_id, q.since, writable).await {
            tracing::warn!(error = %e, vault_id = %vault_id, "sync socket exited with error");
        }
    }))
}

async fn run_socket(
    socket: WebSocket,
    state: AppState,
    auth: AuthCtx,
    vault_id: Uuid,
    since: i64,
    writable: bool,
) -> anyhow::Result<()> {
    let (mut tx, mut rx) = socket.split();

    // Subscribe before replay so we don't lose any op produced during
    // the replay window.
    let mut live = state.sync_hub.subscribe(vault_id).await;

    // ---- 1. Replay history ----
    let mut cursor = since;
    loop {
        let chunk: Vec<OpLogEntry> = sqlx::query_as!(
            OpLogEntry,
            r#"
            SELECT id, vault_id, author_user_id, device_id, target_kind,
                   payload, client_ts_ms, created_at
            FROM audire.op_log
            WHERE vault_id = $1 AND id > $2
            ORDER BY id ASC
            LIMIT $3
            "#,
            vault_id,
            cursor,
            REPLAY_CHUNK,
        )
        .fetch_all(&state.db)
        .await?;
        if chunk.is_empty() {
            break;
        }
        cursor = chunk.last().map(|o| o.id).unwrap_or(cursor);
        send_msg(&mut tx, &SyncMessage::Ops { vault_id, ops: chunk }).await?;
    }
    let last_op_id = sqlx::query_scalar!(
        r#"SELECT last_op_id FROM audire.vaults WHERE id = $1"#,
        vault_id
    )
    .fetch_one(&state.db)
    .await?;
    send_msg(&mut tx, &SyncMessage::CaughtUp { vault_id, last_op_id }).await?;

    // ---- 2. Live duplex ----
    loop {
        tokio::select! {
            // Inbound from client.
            next = rx.next() => {
                let Some(msg) = next else { break; };
                let msg = msg?;
                let frame: SyncMessage = match msg {
                    Message::Text(s) => serde_json::from_str(&s)?,
                    Message::Binary(b) => serde_json::from_slice(&b)?,
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => break,
                };
                match frame {
                    SyncMessage::Append { vault_id: vid, op } => {
                        if vid != vault_id {
                            send_err(&mut tx, "wrong_vault", "append target mismatch").await?;
                            continue;
                        }
                        if !writable {
                            send_err(&mut tx, "forbidden", "reader role cannot append").await?;
                            continue;
                        }
                        let stored = insert_op(&state, vault_id, auth.user_id, &op).await?;
                        send_msg(
                            &mut tx,
                            &SyncMessage::Ack {
                                local_id: op.local_id,
                                op_id: stored.id,
                                client_ts_ms: stored.client_ts_ms,
                            },
                        )
                        .await?;
                        state.sync_hub.publish(stored).await;
                    }
                    SyncMessage::Ping => send_msg(&mut tx, &SyncMessage::Pong).await?,
                    SyncMessage::Pong => {}
                    // Other variants are server→client only; ignore if a
                    // client sends them.
                    _ => {}
                }
            }
            // Live broadcast for this vault.
            recv = live.recv() => {
                match recv {
                    Ok(op) => {
                        // Skip echoing the sender's own appends back to them.
                        if op.author_user_id == auth.user_id {
                            continue;
                        }
                        send_msg(&mut tx, &SyncMessage::Live { vault_id, op }).await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Slow consumer — ask the client to reconnect for
                        // a fresh replay rather than dropping ops silently.
                        send_err(&mut tx, "lagged", "fell behind live stream — reconnect").await?;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}

async fn insert_op(
    state: &AppState,
    vault_id: Uuid,
    author_user_id: Uuid,
    op: &NewOp,
) -> anyhow::Result<OpLogEntry> {
    let row = sqlx::query!(
        r#"
        INSERT INTO audire.op_log
            (vault_id, author_user_id, device_id, target_kind, payload, client_ts_ms)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, created_at
        "#,
        vault_id,
        author_user_id,
        op.device_id,
        op.target_kind,
        &op.payload,
        op.client_ts_ms,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(OpLogEntry {
        id: row.id,
        vault_id,
        author_user_id,
        device_id: op.device_id,
        target_kind: op.target_kind.clone(),
        payload: op.payload.clone(),
        client_ts_ms: op.client_ts_ms,
        created_at: row.created_at,
    })
}

async fn send_msg<S>(tx: &mut S, m: &SyncMessage) -> anyhow::Result<()>
where
    S: SinkExt<Message, Error = axum::Error> + Unpin,
{
    let body = serde_json::to_string(m)?;
    tx.send(Message::Text(body)).await?;
    Ok(())
}

async fn send_err<S>(tx: &mut S, code: &str, msg: &str) -> anyhow::Result<()>
where
    S: SinkExt<Message, Error = axum::Error> + Unpin,
{
    send_msg(
        tx,
        &SyncMessage::Error {
            code: code.into(),
            message: msg.into(),
        },
    )
    .await
}
