-- story: e02s01
CREATE TABLE repositories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name CITEXT NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'public' CHECK (visibility IN ('public', 'private')),
    trunk TEXT NOT NULL DEFAULT 'trunk',
    merge_setting TEXT NOT NULL DEFAULT 'squash' CHECK (merge_setting IN ('squash', 'merge_commit')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_id, name)
);
