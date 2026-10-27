-- story: e25s01
-- profile fields the settings page writes; bky's slice-0 scope
-- (the later e21 user_prefs migration rebases on top of this one)
ALTER TABLE users ADD COLUMN display_name TEXT;
ALTER TABLE users ADD COLUMN theme TEXT NOT NULL DEFAULT 'system'
    CHECK (theme IN ('dark', 'light', 'system'));
