# Audire cloud architecture (Sync v1)

> **Status:** locked for v1. 2026-05-01.
> **Phase:** scaffolding done; auth + personal-vault sync implementation in progress.

This document is the canonical specification for **Audire Sync** — the
optional, paid, end-to-end-encrypted cloud service that syncs transcripts
and notes across a user's devices and (Phase 2) lets users share folder
vaults with their organisation.

If you're reading this to write code: every decision below is binding
unless explicitly revisited in a follow-up doc with a newer date.

---

## 1. Foundational principles (locked)

| | Decision | Source |
|-|-|-|
| Privacy | Audio is **never** persisted, anywhere. Sync transports transcripts + notes only. | `docs/billing-model.md`, README |
| Local-first | Local app is fully featured without any account. Sync is opt-in. | conversation 2026-05-01 |
| E2E | Every byte of vault content is encrypted on-device before leaving it. Server stores ciphertext + opaque metadata only. | conversation 2026-05-01 |
| Recovery | Argon2id-derived passphrase + one-time recovery key (Obsidian model). Lose both = lose data. | conversation 2026-05-01 |
| Tenancy | Soft (`org_id` column + Postgres RLS). | conversation 2026-05-01 |
| Auth | **Stack Auth** (OSS, runs in our Neon DB). | conversation 2026-05-01 |
| Sync model | WebSocket + per-vault append-only op log, last-write-wins per-row reconciliation. No CRDT. | conversation 2026-05-01 |
| Stack | Rust + Axum + sqlx on Fly.io, Postgres on Neon, shared types crate. | conversation 2026-05-01 |
| Billing | BYOK across every tier; storage is what we charge for. | `docs/billing-model.md` |
| Pricing | Free local · Sync $4 · Sync Plus $8 (Obsidian-shaped). | `website/pricing.html` |
| Sharing | **Folder-level vaults** (per-meeting/per-note overrides deferred to v2+). | conversation 2026-05-01 |

---

## 2. System layout

```
                                   ┌──────────────────────────────┐
   ┌───────────────────┐           │                              │
   │  Audire desktop   │           │  audire.app marketing site   │
   │  (Tauri + Rust)   │           │  (static, Cloudflare Pages)  │
   └────────┬──────────┘           │                              │
            │                      └──────────────────────────────┘
            │  HTTPS (REST + WSS)
            ▼
   ┌──────────────────────────────────────────────────────┐
   │                  Fly.io (audire-server)              │
   │  Axum 0.7 + tokio + sqlx + tokio-tungstenite         │
   │  - Stack Auth JWT verification                       │
   │  - Vault CRUD                                        │
   │  - Sync WebSocket fan-out                            │
   └─────────────┬────────────────────────────────────────┘
                 │                          ▲
                 │ Postgres (sqlx)          │ Stack Auth JWKS
                 ▼                          │
   ┌──────────────────────┐    ┌────────────────────────────┐
   │       Neon           │    │  Stack Auth (in our Neon)  │
   │ (Postgres branches)  │    │  users, sessions, orgs,    │
   │  vaults, op_log,     │    │  invites — managed by      │
   │  key_envelopes, etc. │    │  Stack Auth's own schema.  │
   └──────────────────────┘    └────────────────────────────┘
```

- **Server is dumb by design.** It stores ciphertext blobs per vault, fans
  them out to subscribed devices, and authenticates users. It cannot
  decrypt anything inside a vault.
- **Stack Auth lives in the same Neon DB** under its own schema. Our
  application data joins `users.id` to `stack_auth.user_id` (or whichever
  column name Stack Auth uses for primary user identity).
- **Audio never enters this picture.** It's not in the data model, not in
  the wire protocol, not in any code path.

---

## 3. Data model

All tables live in the `audire` schema in our Neon database. Stack Auth's
tables live in `stack_auth` (or its default schema).

### `audire.users`

A thin local mirror of Stack Auth's user record so we can JOIN against
our own data without round-tripping to Stack Auth on every query.

```sql
CREATE TABLE audire.users (
    id              UUID PRIMARY KEY,                -- mirrors stack_auth.user_id
    email           TEXT NOT NULL,
    public_key      BYTEA NOT NULL,                  -- X25519 public key, used to wrap vault keys for sharing
    recovery_key_id UUID,                            -- references audire.recovery_keys
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### `audire.recovery_keys`

A user's recovery key envelope. Issued once at first sign-in, never
shown again. The user must store the key offline.

```sql
CREATE TABLE audire.recovery_keys (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    -- The user's master key (KEK) wrapped with a recovery-key-derived
    -- key. Server never sees the recovery key itself.
    wrapped_kek     BYTEA NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### `audire.vaults`

A vault is a folder-shaped sync unit. v1 supports a personal vault
(implicit, created on first sign-in) and named vaults the user creates.
A vault corresponds 1:1 to a folder in the desktop app's existing
folder list.

```sql
CREATE TABLE audire.vaults (
    id              UUID PRIMARY KEY,
    name_ciphertext BYTEA NOT NULL,                  -- name encrypted with vault key (server can't read)
    owner_user_id   UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    org_id          UUID,                            -- soft tenancy; null for personal vaults
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Monotonically increasing op_log offset, server-assigned. Clients
    -- pull deltas with `WHERE id > since_op_id`.
    last_op_id      BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX vaults_owner_idx ON audire.vaults(owner_user_id);
CREATE INDEX vaults_org_idx ON audire.vaults(org_id) WHERE org_id IS NOT NULL;
```

### `audire.vault_members`

Who has access to which vault, and how their copy of the vault key is
wrapped. Adding/removing members rotates the vault key.

```sql
CREATE TABLE audire.vault_members (
    vault_id        UUID NOT NULL REFERENCES audire.vaults(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    -- Vault key wrapped with this user's public key (X25519 sealed box).
    wrapped_vault_key BYTEA NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'reader')),
    invited_by      UUID REFERENCES audire.users(id),
    invited_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    accepted_at     TIMESTAMPTZ,
    PRIMARY KEY (vault_id, user_id)
);
```

### `audire.op_log`

The append-only stream of changes for a vault. Each row is a single
encrypted operation: an `INSERT`, `UPDATE`, or `DELETE` of a logical
record (note, transcript segment, structured-note item, etc.) inside
the vault. The server never decrypts the payload; the table name and
the `target_kind` field are used only by the **client** when replaying.

```sql
CREATE TABLE audire.op_log (
    id              BIGSERIAL PRIMARY KEY,
    vault_id        UUID NOT NULL REFERENCES audire.vaults(id) ON DELETE CASCADE,
    -- Author user_id is plaintext for billing/abuse audits but the
    -- payload is opaque ciphertext.
    author_user_id  UUID NOT NULL REFERENCES audire.users(id),
    -- The client device that produced the op. Plaintext for audit only.
    device_id       UUID NOT NULL,
    -- Logical kind (e.g. "note", "segment", "folder", "meeting"). Used
    -- by the client to dispatch the decrypted payload to the right
    -- local-DB handler. It's plaintext but does not reveal content.
    target_kind     TEXT NOT NULL,
    -- Opaque encrypted payload. The crypto envelope (nonce + AEAD tag)
    -- is included inside the bytes; server treats it as opaque.
    payload         BYTEA NOT NULL,
    -- Monotonic clock from the client at op-creation, used for LWW
    -- reconciliation when two devices touched the same row offline.
    client_ts_ms    BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX op_log_vault_idx ON audire.op_log(vault_id, id);
```

### `audire.orgs` (Phase 2)

Phase 2 introduces shared org vaults. Stack Auth has its own teams/orgs
primitives — we mirror only the foreign key.

```sql
CREATE TABLE audire.orgs (
    id              UUID PRIMARY KEY,                -- mirrors stack_auth.team_id
    name            TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Row-Level Security

Every table above gets RLS turned on with policies that restrict access
to the authenticated user's vaults via `vault_members`. Detailed
policies live in `server/migrations/` once the migrations are written.

---

## 4. Cryptography

### Keys

- **Master Key (KEK)** — per-user. 32-byte symmetric key, never leaves
  the device unwrapped. Derived from the user's passphrase using
  Argon2id (memlimit=64 MB, opslimit=3, salt = per-user random,
  stored alongside `audire.users` row). The Argon2id parameters and
  salt are stored on the server; the passphrase itself is never
  transmitted.
- **Vault Key** — per-vault. 32-byte symmetric key (XChaCha20-Poly1305).
  Generated client-side at vault creation, wrapped with the owner's
  KEK and uploaded as `wrapped_vault_key` for the owner's
  `vault_members` row.
- **User Sharing Keypair** — per-user. X25519 keypair. Public half is
  uploaded to `audire.users.public_key`. Private half is wrapped with
  KEK and stored locally (never uploaded).
- **Recovery Key** — per-user. 32-byte random key shown to the user
  exactly once. Used to wrap a copy of KEK that lives on the server
  (`audire.recovery_keys.wrapped_kek`).

### Wrapping flow at signup

1. User picks a passphrase. Client derives KEK via Argon2id.
2. Client generates an X25519 keypair. Wraps the private key with KEK.
3. Client generates a 32-byte recovery key. Wraps a copy of KEK with it.
4. Client uploads `users.public_key` and `recovery_keys.wrapped_kek`.
5. Client displays the recovery key to the user once with a "save this
   somewhere safe; we cannot recover it for you" warning.
6. Client creates the user's personal vault (see below).

### Vault creation

1. Client generates a 32-byte vault key.
2. Client encrypts the vault name with the vault key.
3. Client wraps the vault key with the owner's KEK.
4. Client posts to `POST /v1/vaults` with `{ name_ciphertext,
   wrapped_vault_key, role: "owner" }`. Server inserts a `vaults` row
   and a `vault_members` row in a transaction.

### Sharing a vault

1. Inviting client fetches recipient's `users.public_key` from server.
2. Inviting client unwraps the vault key with their own KEK, then wraps
   it again as a sealed box to the recipient's public key.
3. Client posts `POST /v1/vaults/:id/members` with `{ user_id,
   wrapped_vault_key, role }`. Server inserts a `vault_members` row.
4. Recipient device unwraps with its private X25519 key on next sync.

### Removing a member

1. Owner client generates a new vault key.
2. Owner client re-wraps the new vault key for every remaining member
   (using each member's public key) and re-encrypts the vault name.
3. Owner client posts `POST /v1/vaults/:id/rotate-key` with the new
   wraps. Server replaces the existing `vault_members.wrapped_vault_key`
   for kept members in a transaction and deletes the removed member's
   row.
4. New ops from now on use the new vault key. Old ops remain encrypted
   under the old key — kept members already had the old key locally;
   the removed member can no longer decrypt new ops.

### Op encryption

Each op payload is `XChaCha20-Poly1305(vault_key, nonce, plaintext)`,
where `plaintext` is JSON of the form:

```json
{
  "kind": "note",
  "id": "note-uuid",
  "operation": "upsert",
  "row": { "title": "...", "body": "...", "updated_at_ms": 1735689600000 }
}
```

The server stores `nonce || ciphertext || tag` as `payload`. AAD
includes `vault_id`, `author_user_id`, `device_id`, `target_kind`.

---

## 5. Sync wire protocol

REST is used for vault CRUD. The actual stream of changes goes over a
WebSocket per vault.

### Connection

`wss://server.audire.app/v1/sync/:vault_id?since=<op_id>`

Authenticated by a Stack Auth JWT in the `Sec-WebSocket-Protocol`
header (`audire.v1.<base64-jwt>`). Server validates against Stack Auth
JWKS and verifies the user has a `vault_members` row for `vault_id`.

### Messages

Both directions speak the same `SyncMessage` enum (see
`shared/src/lib.rs::SyncMessage`):

```rust
pub enum SyncMessage {
    /// Server → client on connect: snapshot of all ops since the
    /// `since` cursor in the URL. Sent as one or more `Ops` frames
    /// then a `Caught Up { last_op_id }` frame.
    Ops { vault_id: Uuid, ops: Vec<OpLogEntry> },
    CaughtUp { vault_id: Uuid, last_op_id: i64 },

    /// Client → server: append a new op produced locally.
    Append { vault_id: Uuid, op: NewOp },
    /// Server → client: ack with the assigned op id.
    Ack { local_id: Uuid, op_id: i64 },

    /// Server → client: live broadcast of an op produced by another
    /// device on the same vault.
    Live { vault_id: Uuid, op: OpLogEntry },

    /// Either direction: keep-alive.
    Ping,
    Pong,
}
```

### Reconciliation

Conflict between two devices that touched the same row while offline is
resolved by **last-write-wins on `client_ts_ms`** at replay time on the
client. Both ops are still stored on the server (full history), but the
client's local DB ends up with the higher-timestamp version. We accept
that this can lose work in a multi-device-offline edit scenario; CRDT
upgrade is on the v2 roadmap if real users complain.

---

## 6. Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET`  | `/v1/health` | Liveness (no auth). |
| `POST` | `/v1/users/me` | First-sign-in: create our `users` row from Stack Auth claims; accept `public_key` + recovery envelope. Idempotent. |
| `GET`  | `/v1/users/me` | Read own profile. |
| `GET`  | `/v1/users/lookup?email=` | Look up another user's public key by email (only succeeds if the looked-up user exists in our DB). |
| `GET`  | `/v1/vaults` | List vaults the user is a member of. |
| `POST` | `/v1/vaults` | Create a new vault (personal or org). |
| `GET`  | `/v1/vaults/:id` | Get one vault's metadata + the caller's wrapped vault key. |
| `POST` | `/v1/vaults/:id/members` | Invite a user (Phase 2). |
| `POST` | `/v1/vaults/:id/rotate-key` | Rotate vault key after member removal (Phase 2). |
| `DELETE` | `/v1/vaults/:id/members/:user_id` | Remove a member (Phase 2). |
| `WS`   | `/v1/sync/:id` | Sync stream described above. |

All non-`/health` endpoints require a valid Stack Auth JWT.

---

## 7. Phased rollout

| Phase | Scope | Status |
|-------|-------|--------|
| **0** — foundation | Architecture doc, server scaffold, shared types crate, migrations, Stack Auth integration | **In progress** |
| **1** — personal vault sync | `POST /v1/users/me`, `POST /v1/vaults`, `GET /v1/vaults`, `WS /v1/sync/:id`, desktop sign-in flow, op log append/replay for notes + transcripts. **One vault per user.** | Next |
| **2** — multiple vaults + org sharing | UI for creating named vaults, mapping app folders to vaults, member invite/remove, key rotation, org primitives | After Phase 1 lands |
| **3** — version history | Server-side retention policy honoring tier (1 month / 12 months), `GET /v1/vaults/:id/history` endpoint, restore UI | After Phase 2 |
| **4** — Audire Publish | Static-site renderer for shared notes, separate Fly app, custom domain support | Independent track |
| **5** — Managed AI tier (optional) | Stripe metered billing + key broker. **Reserved**, not committed. | See `docs/billing-model.md` |

---

## 8. Operations

### Repos / dirs

- `server/` — Cargo workspace at the root of the audire repo.
  - `server/server/` — Axum binary.
  - `server/shared/` — types crate, also referenced by `src-tauri/`.
  - `server/migrations/` — sqlx migrations.
  - `server/Dockerfile` — minimal Rust + distroless runtime.
  - `server/fly.toml` — Fly.io config.
- Desktop sync code lives in `src-tauri/src/sync/`.

### Environment variables

| Variable | Where | Notes |
|----------|-------|-------|
| `DATABASE_URL` | server | Neon connection string. |
| `STACK_AUTH_PROJECT_ID` | server | From Stack Auth dashboard. |
| `STACK_AUTH_PUBLISHABLE_CLIENT_KEY` | server + desktop | Front-end safe. |
| `STACK_AUTH_SECRET_SERVER_KEY` | server only | Never bundled with desktop. |
| `STACK_AUTH_JWKS_URL` | server | `https://api.stack-auth.com/api/v1/projects/<project>/.well-known/jwks.json`. |
| `RUST_LOG` | server | Defaults to `audire_server=info,tower_http=info`. |
| `BIND_ADDR` | server | Defaults to `0.0.0.0:8080`. |

### Deploy

```sh
cd server
fly deploy
```

Neon migrations run automatically on container start (one-shot
`sqlx::migrate!()` call in `main`).

### Security review checkpoints

Before Phase 1 ships:

- [ ] JWT verification against Stack Auth JWKS confirmed in unit tests
      (signed-by-someone-else, expired, wrong audience, wrong issuer
      all return 401).
- [ ] RLS policies tested with a deliberately-mis-claimed JWT to make
      sure the server cannot serve another user's vault even if the
      application code has a bug.
- [ ] Rate limiting on `/v1/users/lookup` to prevent enumeration.
- [ ] Audit log for every `POST /v1/vaults/:id/members` and
      `/rotate-key` (Phase 2 prerequisite).
- [ ] No log line contains a JWT, a recovery key, or any plaintext
      vault content. Asserted in tests.

---

## 9. Decisions explicitly *not* in v1

- CRDT-based merging.
- Real-time presence / cursors.
- Server-side full-text search of encrypted content (impossible by
  construction; client-side index is the v2 plan).
- Self-hosted server release. Source is open-sourced; supported
  deploy stays as `fly deploy` until usage justifies hardening the
  one-click self-host story.
- Mobile clients.
