#!/usr/bin/env bash
# story: e02s01 — e2e: register, create repo, jj git push with token, jj git clone, assert contents.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3900}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"; kill "${SVPID:-0}" 2>/dev/null || true' EXIT

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
JJHUB_STORAGE_ROOT="$TMP/repos" JJHUB_PORT="$PORT" DATABASE_URL="$DB_URL" ./target/debug/server &
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
  -d "name=e2e-demo&visibility=public&_csrf=$CSRF")
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
  echo "hello jjhub" > hello.txt
  jj describe -m "e2e init"
  jj bookmark create trunk 2>/dev/null || jj bookmark set trunk --allow-backwards 2>/dev/null || true
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  jj git push --all >/dev/null
)

echo "== jj git clone =="
jj git clone "$BASE/git/${OWNER_NAME}.git" "$TMP/clone" >/dev/null
test -f "$TMP/clone/hello.txt" || { echo "clone missing hello.txt"; exit 1; }
grep -q "hello jjhub" "$TMP/clone/hello.txt"

echo "== anonymous clone of public repo =="
jj git clone "$BASE/git/${OWNER_NAME}.git" "$TMP/anon-clone" >/dev/null

echo "E2E PASS"
