#!/usr/bin/env bash
# story: e05s03 — e2e: the review proxy authenticates, forwards identity to a
# real kata sidecar in trust-forwarded-header mode, and never lets a client
# set that identity itself.
#
# Requires a `kata` binary on PATH (build from the pinned commit, same as
# scripts/e2e-sidecar.sh).
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3902}"
KATA_PORT="${KATA_E2E_PORT:-7880}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
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
  KATA_UPSTREAM="http://127.0.0.1:${KATA_PORT}" \
  ./target/debug/server &
SVPID=$!
for _ in $(seq 1 50); do
  curl -sf "$BASE/healthz" >/dev/null && break
  sleep 0.1
done

EMAIL="proxy-e2e-$(date +%s%N)@example.com"
CJ="$TMP/cookies"

echo "== register + create repo + push =="
curl -sf -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" \
  --data-urlencode "password_confirm=correct horse battery" >/dev/null

# session-authed POSTs carry the synchronizer token
SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")
LOC=$(curl -sf -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d "name=proxy-demo&visibility=public&_csrf=$CSRF")
OWNER_NAME="${LOC#"$BASE"/}"
NEW_TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d "label=e2e&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
AUTH_URL="http://x-access-token:$NEW_TOKEN@127.0.0.1:$PORT/git/${OWNER_NAME}.git"
# CI has no jj identity; a commit written without one can't be pushed
export JJ_USER="e2e" JJ_EMAIL="$EMAIL"
FIXTURE="$TMP/fixture"
jj git init --colocate "$FIXTURE" >/dev/null
(
  cd "$FIXTURE"
  echo "proxy e2e" > hello.txt
  jj describe -m "proxy e2e"
  jj bookmark create trunk 2>/dev/null || true
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  jj git push --all >/dev/null
)

echo "== start kata in trust-forwarded-header mode =="
kata serve --bind "127.0.0.1:${KATA_PORT}" --author "jjhub@localhost" \
  --data "$TMP/kata-data" --workspace "*=$TMP/repos/.meta" \
  --auth-mode trust-forwarded-header \
  --auth-trusted-header X-Review-Author \
  --auth-trust-upstream 127.0.0.1/32 \
  >"$TMP/kata.log" 2>&1 &
KATA_PID=$!
for _ in $(seq 1 50); do
  curl -sf "http://127.0.0.1:${KATA_PORT}/api/whoami" >/dev/null 2>&1 && break
  sleep 0.1
done

echo "== unauthenticated is login redirect =="
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/review/${OWNER_NAME}/")
test "$CODE" = "303" || { echo "expected 303, got $CODE" >&2; exit 1; }

echo "== logged-in reaches sidecar with OUR identity =="
# forge the identity header on the client; the proxy must replace it
WHOAMI=$(curl -s -b "$CJ" -H "X-Review-Author: evil@example.com" \
  "$BASE/review/${OWNER_NAME}/api/whoami")
echo "$WHOAMI"
echo "$WHOAMI" | grep -q "$EMAIL" || {
  echo "sidecar did not see the authenticated identity ($EMAIL)" >&2; exit 1;
}
echo "$WHOAMI" | grep -q "evil@example.com" && {
  echo "client-supplied identity leaked through" >&2; exit 1;
}

echo "E2E-REVIEW-PROXY PASS"
