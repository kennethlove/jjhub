-- story: e03s01
CREATE TABLE merge_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repo_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    change_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'merged', 'abandoned')),
    merge_commit_sha TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- story: e03s01 — invariant 1: at most ONE open MR per (repo, change-id).
-- Abandoned/merged rows don't block a future open MR.
CREATE UNIQUE INDEX merge_requests_open_unique
    ON merge_requests (repo_id, change_id)
    WHERE state = 'open';

CREATE INDEX merge_requests_repo_state ON merge_requests (repo_id, state);

-- story: e03s01 — invariant 5: revisions are immutable, append-only.
CREATE TABLE revisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    mr_id UUID NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    commit_id TEXT NOT NULL,
    pushed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (mr_id, commit_id)
);
