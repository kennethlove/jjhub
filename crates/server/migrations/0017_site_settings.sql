-- story: e32s01 — registration is a toggle, not an env var: the
-- setting survives restarts and flips from the operator console.
CREATE TABLE site_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- story: e32s01 — a registration flip is a site-level action with no
-- target user, so the audit row accepts a null target and the new
-- actions join the closed set.
ALTER TABLE moderation_actions ALTER COLUMN target_id DROP NOT NULL;
ALTER TABLE moderation_actions DROP CONSTRAINT moderation_actions_action_check;
ALTER TABLE moderation_actions ADD CONSTRAINT moderation_actions_action_check
    CHECK (action IN (
        'suspend', 'unsuspend', 'registration_close', 'registration_open'
    ));
