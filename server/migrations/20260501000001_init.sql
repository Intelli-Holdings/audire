-- Audire Sync — initial schema (Phase 0 + Phase 1).
-- See docs/cloud-architecture.md § 3 for the design rationale.
--
-- Privacy invariant: this server never stores plaintext vault content.
-- Every column whose name ends in `_ciphertext`, `wrapped_*`, or
-- `payload` is opaque to the server. Code review must reject any change
-- that reads these columns server-side.

CREATE SCHEMA IF NOT EXISTS audire;

CREATE TABLE audire.users (
    id              UUID PRIMARY KEY,
    email           TEXT NOT NULL,
    public_key      BYTEA NOT NULL,
    recovery_key_id UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_idx ON audire.users (lower(email));

CREATE TABLE audire.recovery_keys (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    wrapped_kek     BYTEA NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX recovery_keys_user_idx ON audire.recovery_keys (user_id);

ALTER TABLE audire.users
    ADD CONSTRAINT users_recovery_fk
    FOREIGN KEY (recovery_key_id) REFERENCES audire.recovery_keys(id);

CREATE TABLE audire.orgs (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE audire.vaults (
    id              UUID PRIMARY KEY,
    name_ciphertext BYTEA NOT NULL,
    owner_user_id   UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    org_id          UUID REFERENCES audire.orgs(id) ON DELETE CASCADE,
    last_op_id      BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX vaults_owner_idx ON audire.vaults (owner_user_id);
CREATE INDEX vaults_org_idx ON audire.vaults (org_id) WHERE org_id IS NOT NULL;

CREATE TABLE audire.vault_members (
    vault_id          UUID NOT NULL REFERENCES audire.vaults(id) ON DELETE CASCADE,
    user_id           UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    wrapped_vault_key BYTEA NOT NULL,
    role              TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'reader')),
    invited_by        UUID REFERENCES audire.users(id),
    invited_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    accepted_at       TIMESTAMPTZ,
    PRIMARY KEY (vault_id, user_id)
);
CREATE INDEX vault_members_user_idx ON audire.vault_members (user_id);

CREATE TABLE audire.op_log (
    id              BIGSERIAL PRIMARY KEY,
    vault_id        UUID NOT NULL REFERENCES audire.vaults(id) ON DELETE CASCADE,
    author_user_id  UUID NOT NULL REFERENCES audire.users(id),
    device_id       UUID NOT NULL,
    target_kind     TEXT NOT NULL,
    payload         BYTEA NOT NULL,
    client_ts_ms    BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX op_log_vault_id_idx ON audire.op_log (vault_id, id);
CREATE INDEX op_log_vault_created_idx ON audire.op_log (vault_id, created_at);

-- last_op_id maintenance — keep audire.vaults.last_op_id in sync with the
-- highest op_log.id for each vault. We do this in a trigger rather than
-- in application code so concurrent appends from different connections
-- can't race a "max(id)" SELECT with another append.
CREATE OR REPLACE FUNCTION audire.bump_last_op_id() RETURNS TRIGGER AS $$
BEGIN
    UPDATE audire.vaults
       SET last_op_id = NEW.id,
           updated_at = now()
     WHERE id = NEW.vault_id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER op_log_bump_last_id
AFTER INSERT ON audire.op_log
FOR EACH ROW EXECUTE FUNCTION audire.bump_last_op_id();
