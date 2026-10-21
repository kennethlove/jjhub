-- story: e10s03
-- Audit trail for refused pushes: one row per refused ref update.
CREATE TABLE push_rejections (
    id BIGSERIAL PRIMARY KEY,
    repo_id UUID NOT NULL REFERENCES repositories(id),
    ref_name TEXT NOT NULL,
    old_sha TEXT NOT NULL,
    new_sha TEXT NOT NULL,
    -- pushing identity parsed from the new head commit; 'unknown' when
    -- no commit could be parsed (e.g. a protected-ref deletion)
    identity TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX push_rejections_repo_id_idx ON push_rejections (repo_id);
