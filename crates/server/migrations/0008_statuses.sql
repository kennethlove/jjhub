-- story: e04s02 — external CI reports one status per (revision, context);
-- re-reporting the same context replaces the previous state.
CREATE TABLE statuses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    mr_id UUID NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    revision_id UUID NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    context TEXT NOT NULL CHECK (length(trim(context)) > 0),
    state TEXT NOT NULL
        CHECK (state IN ('pending', 'success', 'failure', 'error')),
    target_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (revision_id, context)
);

CREATE INDEX statuses_revision ON statuses (revision_id);
