-- story: e21s01 — user preferences, the details-completion marker,
-- and the one-time plaintext handoff for the signup details screen
ALTER TABLE users
  ADD COLUMN display_name TEXT,
  ADD COLUMN theme TEXT NOT NULL DEFAULT 'system'
    CHECK (theme IN ('dark', 'light', 'system')),
  ADD COLUMN token_details_completed_at TIMESTAMPTZ;

-- the signup token's plaintext lives here only until the details
-- screen reads it once; the hash-only tokens row stays authoritative
CREATE TABLE token_details_handoff (
  user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  token_id UUID NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
  plaintext TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
