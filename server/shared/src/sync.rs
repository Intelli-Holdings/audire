//! WebSocket sync protocol — see `docs/cloud-architecture.md` § 5.
//!
//! Both directions speak `SyncMessage`. The server is dumb: it never
//! peeks inside an op's `payload`. Conflict resolution is client-side
//! (last-write-wins on `client_ts_ms`).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{NewOp, OpLogEntry};

/// One frame on the sync WebSocket. Tagged with `type` for trivial
/// round-tripping in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncMessage {
    /// Server → client: a chunk of historical ops since the `since`
    /// cursor in the URL. May be sent in multiple frames if the
    /// backlog is large.
    Ops {
        vault_id: Uuid,
        ops: Vec<OpLogEntry>,
    },
    /// Server → client: signals the historical replay is complete and
    /// the connection has caught up to live.
    CaughtUp {
        vault_id: Uuid,
        last_op_id: i64,
    },

    /// Client → server: append a new op produced locally.
    Append {
        vault_id: Uuid,
        op: NewOp,
    },
    /// Server → client: confirms an `Append` and assigns its server-side
    /// op id.
    Ack {
        local_id: Uuid,
        op_id: i64,
        client_ts_ms: i64,
    },

    /// Server → client: live broadcast of an op produced by another
    /// device on the same vault. `Append` from device A becomes `Live`
    /// for every other connected device on the same vault.
    Live {
        vault_id: Uuid,
        op: OpLogEntry,
    },

    /// Server → client: an error specific to this WebSocket frame
    /// (e.g. unauthorised vault, malformed op). Closing the socket is
    /// reserved for fatal cases (auth failure, internal error).
    Error {
        code: String,
        message: String,
    },

    /// Either direction: keep-alive. Servers behind Fly's edge timeout
    /// idle WebSockets at ~5 min, so the desktop client pings every
    /// 4 min while connected.
    Ping,
    Pong,
}
