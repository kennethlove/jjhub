-- story: e29s01 — operator designation and reversible suspension.
-- The role starts false for everyone: the deployment env names the
-- bootstrap operator before any account exists; the column promotes.
ALTER TABLE users ADD COLUMN is_operator BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN suspended_at TIMESTAMPTZ;

-- story: e29s01 — every state change, actor and target kept: this is
-- the moderation record that makes a suspension accountable.
CREATE TABLE moderation_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id UUID NOT NULL REFERENCES users(id),
    target_id UUID NOT NULL REFERENCES users(id),
    action TEXT NOT NULL CHECK (action IN ('suspend', 'unsuspend')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
