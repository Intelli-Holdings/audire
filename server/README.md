# audire-server

Optional cloud sync backend for [Audire](../README.md). Audio never reaches
this service — only encrypted vault metadata, encrypted op-log entries,
and the per-member wrapped vault keys needed for sharing pass through.

This crate is **opt-in**. Audire is fully usable as a local-only desktop
app without ever talking to it.

## Architecture in one paragraph

A user signs in with Stack Auth (run as the free OSS auth provider on the
same Neon project). The desktop client derives a KEK from the user's
passphrase via Argon2id, generates an X25519 keypair, and registers its
public key with `POST /v1/users/me`. Vaults are folders of notes; their
keys are wrapped per-member with each recipient's X25519 public key. The
sync stream is a per-vault WebSocket that replays `op_log` rows since a
client cursor and then keeps both directions live. The full design lives
in [`docs/cloud-architecture.md`](../docs/cloud-architecture.md).

## Layout

```
server/
├── Cargo.toml          workspace
├── Dockerfile          multi-stage build for Fly
├── fly.toml            Fly.io app config (audire-server, iad)
├── migrations/         sqlx migrations (creates schema `audire`)
├── shared/             types shared between server and desktop client
│   └── src/lib.rs      VaultView, OpLogEntry, SyncMessage, …
└── server/
    └── src/
        ├── main.rs     bootstrap, config, router
        ├── auth.rs     Stack Auth JWT + JWKS verification
        ├── error.rs    ApiError → HTTP response
        ├── state.rs    AppState (db pool, sync hub, JWKS cache)
        ├── sync_hub.rs per-vault tokio broadcast fanout
        └── routes/     health, users, vaults, sync (WebSocket)
```

## Local development

You need:

- Rust stable (1.83+)
- Postgres 15+ (Neon works fine; or run `docker run --rm -p 5432:5432 -e POSTGRES_PASSWORD=audire postgres:16`)
- A Stack Auth project — the easiest path is to provision it on your
  existing Neon org. The desktop app will use the same project.

Then:

```bash
cd server
cp .env.example .env
# Fill in DATABASE_URL, STACK_AUTH_PROJECT_ID, STACK_AUTH_JWKS_URL.

# One-time: install the sqlx CLI for offline metadata + migrations.
cargo install sqlx-cli --no-default-features --features postgres

# Apply migrations.
sqlx migrate run --source migrations

# Run the server.
cargo run --bin audire-server
```

Hit `http://localhost:8080/healthz` — you should get
`{"status":"ok",…}`.

### Regenerating sqlx offline metadata

The Docker build uses `SQLX_OFFLINE=true` so it doesn't need DB access.
After changing any SQL in the server, regenerate metadata:

```bash
cd server
cargo sqlx prepare --workspace -- --bin audire-server
git add .sqlx
```

## Deploy to Fly.io

The `fly.toml` is pre-configured. First time:

```bash
fly launch --copy-config --no-deploy           # accept the existing fly.toml
fly secrets set \
    DATABASE_URL="postgres://...neon..." \
    STACK_AUTH_PROJECT_ID="..." \
    STACK_AUTH_JWKS_URL="https://api.stack-auth.com/api/v1/projects/.../.well-known/jwks.json" \
    STACK_AUTH_AUDIENCE="audire-sync"
fly deploy
```

The image runs on a shared-CPU 512 MB machine with `auto_stop_machines`
on, so an idle deployment costs effectively nothing.

### Database

Use Neon. Create a database called `audire` and a role with ownership of
the `audire` schema (the migration creates the schema). Put the
connection string into the `DATABASE_URL` Fly secret.

### Stack Auth

Stack Auth is open-source and runs on the same Neon project. Provision a
project there, copy the project ID and JWKS URL into Fly secrets, and
make sure access tokens you mint carry an `aud` claim equal to
`STACK_AUTH_AUDIENCE` (default `audire-sync`).

## Operational notes

- **Audio never touches this service.** Everything in `op_log.payload`
  is opaque ciphertext from the client's vault key. The schema doesn't
  even define a column for audio.
- The WebSocket `/v1/sync/:vault_id` replays history in chunks of 256
  before going live. Slow consumers get a `lagged` error frame and are
  expected to reconnect with a refreshed `?since=` cursor.
- `vault_members.role` is one of `owner`, `editor`, `reader`. Only
  owners can invite, rotate, or remove members. Readers cannot append.
- Member removal is a two-step client-side flow: owner re-encrypts the
  vault key for everyone who is staying and POSTs to
  `/v1/vaults/:id/rotate-key`, which atomically updates everyone's
  `wrapped_vault_key` and deletes anyone not in the list.

## Endpoints

| Method | Path                                | Notes                                       |
|--------|-------------------------------------|---------------------------------------------|
| GET    | `/healthz`                          | Liveness + DB ping                          |
| POST   | `/v1/users/me`                      | First-sign-in handshake (idempotent)        |
| GET    | `/v1/users/me`                      | Caller's profile                            |
| GET    | `/v1/users/lookup?email=…`          | Resolve another Audire user's public key    |
| GET    | `/v1/vaults`                        | Caller's vaults + their wrapped vault keys  |
| POST   | `/v1/vaults`                        | Create vault (caller becomes owner)         |
| GET    | `/v1/vaults/:id`                    | Single vault                                |
| POST   | `/v1/vaults/:id/members`            | Owner invites / re-wraps a member           |
| POST   | `/v1/vaults/:id/rotate-key`         | Owner rotates the vault key after a removal |
| WS     | `/v1/sync/:vault_id?since=<cursor>` | Replay + live op stream                     |
