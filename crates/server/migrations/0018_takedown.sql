-- story: e30s01 — a taken-down repository is never deleted; the wall is
-- reversible visibility, and moderation_actions already records who/when.
ALTER TABLE repositories ADD COLUMN taken_down_at TIMESTAMPTZ;

-- 0017 widened this check once for the registration pair; takedown and
-- restore are the next operator actions that need a row.
ALTER TABLE moderation_actions DROP CONSTRAINT moderation_actions_action_check;
ALTER TABLE moderation_actions ADD CONSTRAINT moderation_actions_action_check
    CHECK (action IN ('suspend', 'unsuspend', 'registration_close', 'registration_open', 'takedown', 'restore'));
