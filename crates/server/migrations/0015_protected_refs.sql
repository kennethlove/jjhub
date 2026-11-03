-- story: e28s01 — user-configurable protected refs. The defaults
-- (refs/heads/{trunk} + refs/tags/*) stay derived in code, never rows,
-- so they are never removable. CHECK puts the trust boundary in the
-- schema: a pattern is refs/ + a refname-safe charset only, so every
-- writer is validated, including ones we never meet, and the panel can
-- render stored patterns without disabling template escaping.
CREATE TABLE IF NOT EXISTS protected_refs (
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    pattern TEXT NOT NULL CHECK (pattern ~ '^refs/[A-Za-z0-9*._/-]+$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (repo_id, pattern)
);
