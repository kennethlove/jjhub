#!/usr/bin/env bash
# story: e03s03 — e2e: push a two-change jj stack, merge the bottom via the
# endpoint, assert trunk advanced (squash) and the descendant was rebased
# server-side with its change-id preserved.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3904}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
SVPID=""
cleanup() {
  [ -n "$SVPID" ] && kill "$SVPID" 2>/dev/null || true
  if [ "${KEEP_TMP:-0}" = "1" ]; then echo "TMP kept at $TMP"; else rm -rf "$TMP"; fi
}
trap cleanup EXIT

cargo build -q -p server
export JJHUB_STORAGE_ROOT="$TMP/repos" JJHUB_PORT="$PORT" DATABASE_URL="$DB_URL"
./target/debug/server >"$TMP/server.log" 2>&1 &
SVPID=$!
for _ in $(seq 1 50); do curl -sf "$BASE/healthz" >/dev/null && break; sleep 0.1; done

CJ="$TMP/cookies"
EMAIL="cascade-e2e-$(date +%s%N)@example.com"

echo "== register + create repo =="
curl -sf -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" >/dev/null
LOC=$(curl -sf -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d 'name=cascade-demo&visibility=public')
OWNER_NAME="${LOC#"$BASE"/}"
TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d 'label=e2e' -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
AUTH_URL="http://x-access-token:$TOKEN@127.0.0.1:$PORT/git/${OWNER_NAME}.git"
BARE="$TMP/repos/${OWNER_NAME}.git"

echo "== push a two-change stack =="
FIXTURE="$TMP/fixture"
JJ_USER="cascade" JJ_EMAIL="$EMAIL" jj git init --colocate "$FIXTURE" >/dev/null
(
  cd "$FIXTURE"
  # base on trunk; stack A -> C above it
  echo "base" > base.txt
  jj describe -m "base" >/dev/null 2>&1
  jj bookmark set trunk --allow-backwards >/dev/null 2>&1 || jj bookmark create trunk >/dev/null 2>&1
  jj new >/dev/null 2>&1
  echo "one" > one.txt
  jj describe -m "change one" >/dev/null 2>&1
  jj bookmark create change-one >/dev/null 2>&1
  jj new >/dev/null 2>&1
  echo "two" > two.txt
  jj describe -m "change two" >/dev/null 2>&1
  jj bookmark create change-two >/dev/null 2>&1
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  JJ_USER="cascade" JJ_EMAIL="$EMAIL" jj git push --all >/dev/null
)
sleep 1

echo "== open MRs for pushed changes =="
MR_COUNT=$(docker exec jjhub-pg psql -U postgres -d jjhub_test -tAc \
  "SELECT COUNT(*) FROM merge_requests mr \
   JOIN repositories r ON r.id = mr.repo_id \
   JOIN users u ON u.id = r.owner_id \
   WHERE u.email = '$EMAIL' AND mr.state = 'open'")
test "$MR_COUNT" -ge 2 || { echo "expected >=2 open MRs, got $MR_COUNT" >&2; exit 1; }

echo "== merge the bottom MR =="
# the bottom is the open MR whose latest commit is a direct child of the
# trunk head; everything else stacks above it
TRUNK_HEAD_BEFORE=$(git -C "$BARE" rev-parse refs/heads/trunk)
BOTTOM_CHANGE=""
for COMMIT in $(git -C "$BARE" rev-list --all | head -5); do
  PARENT=$(git -C "$BARE" cat-file commit "$COMMIT" | sed -n 's/^parent //p' | head -1)
  if [ "$PARENT" = "$TRUNK_HEAD_BEFORE" ]; then
    BOTTOM_CHANGE=$(docker exec jjhub-pg psql -U postgres -d jjhub_test -tAc \
      "SELECT mr.change_id FROM merge_requests mr \
       JOIN revisions rev ON rev.mr_id = mr.id \
       JOIN repositories r ON r.id = mr.repo_id JOIN users u ON u.id = r.owner_id \
       WHERE u.email = '$EMAIL' AND rev.commit_id LIKE '$COMMIT%' LIMIT 1")
    break
  fi
done
test -n "$BOTTOM_CHANGE" || { echo "no bottom MR found" >&2; exit 1; }
CODE=$(curl -s -b "$CJ" -o /dev/null -w '%{http_code}' -X POST \
  "$BASE/${OWNER_NAME}/pulls/${BOTTOM_CHANGE}/merge")
test "$CODE" = "303" || { echo "merge returned $CODE" >&2; exit 1; }

echo "== trunk advanced, descendant rebased =="
MERGED_STATE=$(docker exec jjhub-pg psql -U postgres -d jjhub_test -tAc \
  "SELECT state FROM merge_requests WHERE change_id = '$BOTTOM_CHANGE'")
test "$MERGED_STATE" = "merged" || { echo "bottom not merged: $MERGED_STATE" >&2; exit 1; }
# the descendant of the merged bottom is the newest other MR in the stack
DESC_SQL="FROM merge_requests mr \
   JOIN repositories r ON r.id = mr.repo_id JOIN users u ON u.id = r.owner_id \
   WHERE u.email = '$EMAIL' AND mr.change_id <> '$BOTTOM_CHANGE' \
   ORDER BY mr.created_at DESC LIMIT 1"
# the descendant of the merged bottom is whichever open MR now sits directly
# on the new trunk head; cascade proof = trunk moved AND some open MR gained
# a rebased revision on it
TRUNK_HEAD_AFTER=$(git -C "$BARE" rev-parse refs/heads/trunk)
test "$TRUNK_HEAD_AFTER" != "$TRUNK_HEAD_BEFORE" || {
  echo "trunk did not move" >&2; exit 1;
}
MAX_REVS=$(docker exec jjhub-pg psql -U postgres -d jjhub_test -tAc \
  "SELECT COALESCE(MAX(cnt), 0) FROM ( \
     SELECT COUNT(*) AS cnt FROM revisions rev \
     JOIN merge_requests mr ON mr.id = rev.mr_id \
     JOIN repositories r ON r.id = mr.repo_id JOIN users u ON u.id = r.owner_id \
     WHERE u.email = '$EMAIL' AND mr.state = 'open' GROUP BY mr.id) counts")
test "$MAX_REVS" -ge 2 || {
  echo "no open MR gained a rebased revision (max $MAX_REVS)" >&2
  grep -i cascade "$TMP/server.log" || true
  exit 1;
}
# the rebased revision becomes the trunk head itself (cascade rebases in
# stack order, last one lands on trunk)
DESC_HEAD=$(docker exec jjhub-pg psql -U postgres -d jjhub_test -tAc \
  "SELECT rev.commit_id FROM revisions rev JOIN merge_requests mr ON mr.id = rev.mr_id \
   JOIN repositories r ON r.id = mr.repo_id JOIN users u ON u.id = r.owner_id \
   WHERE u.email = '$EMAIL' AND mr.state = 'open' \
   ORDER BY rev.pushed_at DESC LIMIT 1")
test "$DESC_HEAD" = "$TRUNK_HEAD_AFTER" || {
  echo "rebased revision is not trunk head: rev=$DESC_HEAD trunk=$TRUNK_HEAD_AFTER" >&2; exit 1;
}
# change-id header survived the server-side rewrite
git -C "$BARE" cat-file commit "$DESC_HEAD" | grep -q '^change-id ' || {
  echo "rebased commit lost its change-id header" >&2; exit 1;
}

echo "E2E-CASCADE-MERGE PASS"
