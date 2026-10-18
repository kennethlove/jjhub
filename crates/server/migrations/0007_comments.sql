-- story: e04s01 — comments are pinned to one immutable revision of one MR.
-- revision_id is a hard FK: a comment can never be re-anchored elsewhere.
-- file/line are the diff anchor when present; NULL = whole-revision thread.
CREATE TABLE comments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    mr_id UUID NOT NULL REFERENCES merge_requests(id) ON DELETE CASCADE,
    revision_id UUID NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    author_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL CHECK (length(trim(body)) > 0),
    file TEXT,
    line INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX comments_revision ON comments (revision_id, created_at);
