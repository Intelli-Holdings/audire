-- Org membership + owner column. Phase 1 of org support.

ALTER TABLE audire.orgs
    ADD COLUMN IF NOT EXISTS owner_user_id UUID REFERENCES audire.users(id) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS audire.org_members (
    org_id      UUID NOT NULL REFERENCES audire.orgs(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES audire.users(id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    invited_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    accepted_at TIMESTAMPTZ,
    PRIMARY KEY (org_id, user_id)
);

CREATE INDEX IF NOT EXISTS org_members_user_idx ON audire.org_members (user_id);
