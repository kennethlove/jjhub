#!/usr/bin/env bash
# story: e03s01 — e2e: a real jj push over HTTP opens a merge request, and a
# foreign-author rewrite of that open change is refused by the pre-receive
# guard with refs left untouched.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3903}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
SVPID=""
cleanup() { [ -n "$SVPID" ] && kill "$SVPID" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
export JJHUB_STORAGE_ROOT="$TMP/repos"
export JJHUB_PORT="$PORT"
export DATABASE_URL="$DB_URL"
./target/debug/server &
SVPID=$!
for _ in $(seq 1 50); do curl -sf "$BASE/healthz" >/dev/null && break; sleep 0.1; done

CJ="$TMP/cookies"
EMAIL="mr-e2e-$(date +%s%N)@example.com"

echo "== register + create repo =="
curl -sf -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" \
  --data-urlencode "password_confirm=correct horse battery" >/dev/null

# session-authed POSTs carry the synchronizer token
SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")
LOC=$(curl -sf -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d "name=mr-demo&visibility=public&_csrf=$CSRF")
OWNER_NAME="${LOC#"$BASE"/}"
TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d "label=e2e&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
AUTH_URL="http://x-access-token:$TOKEN@127.0.0.1:$PORT/git/${OWNER_NAME}.git"
BARE="$TMP/repos/${OWNER_NAME}.git"

echo "== jj git push (author = the registered account) =="
FIXTURE="$TMP/fixture"
# CI has no jj identity; a commit written without one can't be pushed
export JJ_USER="mr-e2e" JJ_EMAIL="$EMAIL"
JJ_USER="mr-e2e" JJ_EMAIL="$EMAIL" jj git init --colocate "$FIXTURE" >/dev/null
(
  cd "$FIXTURE"
  echo "one" > a.txt
  JJ_USER="mr-e2e" JJ_EMAIL="$EMAIL" jj describe -m "change one"
  jj bookmark create trunk 2>/dev/null || true
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  JJ_USER="mr-e2e" JJ_EMAIL="$EMAIL" jj git push --all >/dev/null
)

echo "== MR opened for the pushed change =="
sleep 1 # post-receive upsert is async to the CGI response
MR_ROWS=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM merge_requests mr \
   JOIN repositories r ON r.id = mr.repo_id \
   JOIN users u ON u.id = r.owner_id \
   WHERE u.handle = split_part('$OWNER_NAME', '/', 1) \
     AND r.name = split_part('$OWNER_NAME', '/', 2) \
     AND mr.state = 'open'")
test "$MR_ROWS" -ge 1 || { echo "no open MR after push" >&2; exit 1; }
echo "open MRs: $MR_ROWS"

OLD_REV=$(git -C "$BARE" rev-parse refs/heads/trunk)

echo "== foreign-author rewrite is refused =="
(
  cd "$FIXTURE"
  REV=$(jj log -r trunk --no-graph -T 'commit_id.short()')
  echo "two" > a.txt
  # jj-style foreign rewrite: describe -r preserves the change-id and stamps
  # the foreign identity as committer (author is preserved by jj)
  JJ_USER="mr-e2e" JJ_EMAIL="mallory@evil.example" \
    jj describe -r "$REV" --ignore-immutable -m "mallory rewrite" >/dev/null 2>&1
  if JJ_USER="mr-e2e" JJ_EMAIL="mallory@evil.example" \
      jj git push --all >/dev/null 2>"$TMP/push.err"; then
    echo "foreign-author push was ACCEPTED — invariant 2 broken" >&2; exit 1
  fi
  grep -qi "owner.s identity" "$TMP/push.err" || {
    echo "rejection did not name the author-email rule:" >&2
    cat "$TMP/push.err" >&2; exit 1;
  }
)

echo "== ref untouched =="
NEW_REV=$(git -C "$BARE" rev-parse refs/heads/trunk)
test "$OLD_REV" = "$NEW_REV" || {
  echo "trunk moved despite rejection: $OLD_REV -> $NEW_REV" >&2; exit 1;
}

echo "== re-push by the real author succeeds =="
(
  cd "$FIXTURE"
  # e10s01: a protected trunk is created and advanced, never rewritten —
  # the author amend (a sibling of the accepted head) is refused, so the
  # fixup lands as a child of the accepted remote head: a fast-forward
  # advance that still appends an MR revision.
  jj new trunk@origin -m "author fixup"
  echo "three" > a.txt
  jj bookmark set trunk -r @ --allow-backwards >/dev/null
  JJ_USER="mr-e2e" JJ_EMAIL="$EMAIL" jj git push --all >/dev/null
)
REVISIONS=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM revisions mrrev \
   JOIN merge_requests mr ON mr.id = mrrev.mr_id \
   JOIN repositories r ON r.id = mr.repo_id \
   JOIN users u ON u.id = r.owner_id \
   WHERE u.handle = split_part('$OWNER_NAME', '/', 1)")
test "$REVISIONS" -ge 2 || { echo "re-push did not append a revision ($REVISIONS)" >&2; exit 1; }

echo "E2E-MR-OPEN PASS"
