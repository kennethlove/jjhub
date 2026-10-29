-- story: e22s01 — token permission columns.
--
-- Additive only; the DEFAULT at ADD time IS the backfill (spec note:
-- an explicit UPDATE after NOT NULL DEFAULT is a no-op). tokens rows
-- minted before this migration get {read,write} — the full pre-e22
-- capability — so nothing existing changes behavior.
--
-- Decisions:
--   repo_id ... ON DELETE SET NULL: deleting a repo must neither kill
--     tokens nor block the delete; a pinned token just loses its pin.
--   allowed_inets NULL: no restriction. Non-NULL is an exact-match
--   list checked against the client address (e22s02), fail-closed.

ALTER TABLE tokens ADD COLUMN IF NOT EXISTS scopes TEXT[] NOT NULL DEFAULT '{read,write}';
ALTER TABLE tokens ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;
ALTER TABLE tokens ADD COLUMN IF NOT EXISTS repo_id UUID REFERENCES repositories(id) ON DELETE SET NULL;
ALTER TABLE tokens ADD COLUMN IF NOT EXISTS allowed_inets INET[];
