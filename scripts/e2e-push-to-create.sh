#!/usr/bin/env bash
# story: e19s02 — e2e: a push to a fresh owned path creates the repo
# (PRIVATE) and lands the refs; clone-of-nothing and a foreign
# namespace stay 404 with no row created.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3907}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
SVPID=""
cleanup() {
  [ -n "$SVPID" ] && kill "$SVPID" 2>/dev/null || true
  if [ "${KEEP_TMP:-0}" = "1" ]; then echo "TMP kept at $TMP"; else rm -rf "$TMP"; fi
}
trap cleanup EXIT

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
export JJHUB_STORAGE_ROOT="$TMP/repos" JJHUB_PORT="$PORT" DATABASE_URL="$DB_URL"
JJHUB_EMAIL_CONFIRM=off ./target/debug/server >"$TMP/server.log" 2>&1 &
SVPID=$!
for _ in $(seq 1 50); do curl -sf "$BASE/healthz" >/dev/null && break; sleep 0.1; done

CJ="$TMP/cookies"
EMAIL="push-create-$(date +%s%N)@example.com"

echo "== register (no repo creation) =="
curl -sf -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" \
  --data-urlencode "password_confirm=correct horse battery" >/dev/null

SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")
TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d "label=e2e&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
HANDLE=$(psql "$DB_URL" -tAc "SELECT handle FROM users WHERE email = '$EMAIL'")
AUTH="Basic $(printf 'x-access-token:%s' "$TOKEN" | base64)"

echo "== push to a fresh owned path creates the repo =="
SEED="$TMP/seed"
git init -q -b trunk "$SEED"
(
  cd "$SEED"
  echo "first push" > hello.txt
  git add .
  git -c user.email="$EMAIL" -c user.name="E2E" commit -q -m "init"
)
git -c "http.extraHeader=Authorization: $AUTH" \
  push -q "$BASE/git/$HANDLE/newproj.git" trunk

REFS=$(git -c "http.extraHeader=Authorization: $AUTH" \
  ls-remote "$BASE/git/$HANDLE/newproj.git")
echo "$REFS" | grep -q "refs/heads/trunk" || { echo "FAIL: trunk not visible after push"; exit 1; }
VIS=$(psql "$DB_URL" -tAc \
  "SELECT visibility FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = '$HANDLE' AND r.name = 'newproj'")
[ "$VIS" = "private" ] || { echo "FAIL: push-created repo is '$VIS', want private"; exit 1; }

echo "== clone of nothing stays 404, no row =="
if git -c "http.extraHeader=Authorization: $AUTH" \
    ls-remote "$BASE/git/$HANDLE/never-created.git" >/dev/null 2>&1; then
  echo "FAIL: upload-pack of nothing did not 404"; exit 1
fi
NEVER=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = '$HANDLE' AND r.name = 'never-created'")
[ "$NEVER" = "0" ] || { echo "FAIL: upload-pack probe created a row"; exit 1; }

echo "== foreign namespace stays 404, no row =="
if git -c "http.extraHeader=Authorization: $AUTH" \
    push "$BASE/git/foreigner/newproj.git" trunk >/dev/null 2>&1; then
  echo "FAIL: foreign-namespace push was accepted"; exit 1
fi
ROWS=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM repositories r JOIN users u ON u.id = r.owner_id WHERE u.handle = '$HANDLE'")
[ "$ROWS" = "1" ] || { echo "FAIL: expected exactly 1 row for $HANDLE (newproj), got $ROWS"; exit 1; }

echo "PASS: push-to-create e2e"
