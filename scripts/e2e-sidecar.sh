#!/usr/bin/env bash
# story: e05s02 — e2e: push a repo through jjhub, point kata at the same
# storage, assert kata opens the repo and lists the pushed change.
#
# Proves the whole sidecar contract: the meta dir the app colocates on
# push is a jj workspace kata can load, at the flattened single-level
# layout kata's depth-1 scanner expects.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3901}"
KATA_PORT="${KATA_E2E_PORT:-7879}"
BASE="http://127.0.0.1:${PORT}"
KATA_BASE="http://127.0.0.1:${KATA_PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
STORAGE="${JJHUB_E2E_STORAGE:-}"
TMP="$(mktemp -d)"
KATA_PID=""
SVPID=""
cleanup() {
  [ -n "$KATA_PID" ] && kill "$KATA_PID" 2>/dev/null || true
  [ -n "$SVPID" ] && kill "$SVPID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

command -v kata >/dev/null 2>&1 || {
  echo "kata binary not on PATH — install kata or run via compose" >&2
  exit 1
}

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
JJHUB_STORAGE_ROOT="$TMP/repos" JJHUB_PORT="$PORT" DATABASE_URL="$DB_URL" \
  ./target/debug/server &
SVPID=$!
for _ in $(seq 1 50); do
  curl -sf "$BASE/healthz" >/dev/null && break
  sleep 0.1
done

CJ="$TMP/cookies"
EMAIL="e2e-$(date +%s%N)@example.com"

echo "== register =="
curl -sf -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" \
  --data-urlencode "password_confirm=correct horse battery" >/dev/null

# session-authed POSTs carry the synchronizer token
SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")

echo "== create repo =="
LOC=$(curl -sf -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d "name=sidecar-demo&visibility=public&_csrf=$CSRF")
OWNER_NAME="${LOC#"$BASE"/}"
echo "repo at $OWNER_NAME"

echo "== generate token =="
NEW_TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d "label=e2e&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
test -n "$NEW_TOKEN" || { echo "no token issued"; exit 1; }
AUTH_URL="http://x-access-token:$NEW_TOKEN@127.0.0.1:$PORT/git/${OWNER_NAME}.git"

echo "== jj git push =="
# CI has no jj identity; a commit written without one can't be pushed
export JJ_USER="e2e" JJ_EMAIL="$EMAIL"
FIXTURE="$TMP/fixture"
jj git init --colocate "$FIXTURE" >/dev/null
(
  cd "$FIXTURE"
  echo "hello kata" > hello.txt
  jj describe -m "sidecar e2e"
  jj bookmark create trunk 2>/dev/null || jj bookmark set trunk --allow-backwards 2>/dev/null || true
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  jj git push --all >/dev/null
)

echo "== meta dir colocated at flattened path =="
OWNER="${OWNER_NAME%%/*}"; NAME="${OWNER_NAME##*/}"
META="$TMP/repos/.meta/${OWNER}__${NAME}"
test -d "$META/.jj" || { echo "no colocated meta at $META" >&2; exit 1; }

echo "== start kata sidecar against the same storage =="
kata serve --bind "127.0.0.1:${KATA_PORT}" --author "e2e@example.com" \
  --data "$TMP/kata-data" --workspace "*=$TMP/repos/.meta" \
  >"$TMP/kata.log" 2>&1 &
KATA_PID=$!
for _ in $(seq 1 50); do
  curl -sf "$KATA_BASE/api/whoami" >/dev/null && break
  sleep 0.1
done
curl -sf "$KATA_BASE/api/whoami" >/dev/null || {
  echo "kata did not come up:" >&2; cat "$TMP/kata.log" >&2; exit 1;
}

echo "== kata lists the pushed repo =="
REPOS=$(curl -sf "$KATA_BASE/api/repos")
echo "$REPOS"
echo "$REPOS" | grep -q "${OWNER}__${NAME}" || {
  echo "kata did not list repo ${OWNER}__${NAME}" >&2; exit 1;
}

echo "== kata lists the pushed change =="
LOG=$(curl -sf "$KATA_BASE/api/repos/${OWNER}__${NAME}/browse/log")
echo "$LOG"
echo "$LOG" | grep -q "sidecar e2e" || {
  echo "kata did not list the pushed change" >&2; exit 1;
}

echo "E2E-SIDECAR PASS"
