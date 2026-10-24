#!/usr/bin/env bash
# story: e17s01 — e2e: boot with the SSH listener, seed a repo over HTTP,
# clone it over SSH with a transport token, reject a wrong token, and prove
# both clones hold the same tree.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3901}"
SSH_PORT="${E2E_SSH_PORT:-3922}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"; kill "${SVPID:-0}" 2>/dev/null || true' EXIT

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
JJHUB_STORAGE_ROOT="$TMP/repos" JJHUB_PORT="$PORT" JJHUB_SSH_PORT="$SSH_PORT" \
  JJHUB_SSH_HOST_KEY_DIR="$TMP/host-keys" DATABASE_URL="$DB_URL" ./target/debug/server &
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

SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")

echo "== create repo =="
LOC=$(curl -sf -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d "name=e2e-ssh-demo&visibility=public&_csrf=$CSRF")
OWNER_NAME="${LOC#"$BASE"/}"
echo "repo at $OWNER_NAME"

echo "== generate token =="
NEW_TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d "label=e2e-ssh&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
test -n "$NEW_TOKEN" || { echo "no token issued"; exit 1; }

AUTH_URL="http://x-access-token:$NEW_TOKEN@127.0.0.1:$PORT/git/${OWNER_NAME}.git"

echo "== seed repo over HTTP =="
export JJ_USER="e2e" JJ_EMAIL="$EMAIL"
FIXTURE="$TMP/fixture"
jj git init --colocate "$FIXTURE" >/dev/null
(
  cd "$FIXTURE"
  echo "hello over ssh" > hello.txt
  jj describe -m "e2e ssh init"
  jj bookmark create trunk 2>/dev/null || jj bookmark set trunk --allow-backwards 2>/dev/null || true
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  jj git push --all >/dev/null
)

echo "== ssh clone with token =="
# story: e17s01 — askpass without sshpass: OpenSSH reads SSH_ASKPASS when
# SSH_ASKPASS_REQUIRE=force, tty or not.
printf '#!/bin/sh\necho %s\n' "$NEW_TOKEN" > "$TMP/askpass.sh"
chmod +x "$TMP/askpass.sh"
export SSH_ASKPASS="$TMP/askpass.sh" SSH_ASKPASS_REQUIRE=force
export GIT_SSH_COMMAND="ssh -o UserKnownHostsFile=$TMP/known_hosts -o StrictHostKeyChecking=accept-new -p $SSH_PORT"
git clone "ssh://git@127.0.0.1:$SSH_PORT/${OWNER_NAME}.git" "$TMP/ssh-clone" >/dev/null
grep -q "hello over ssh" "$TMP/ssh-clone/hello.txt"

echo "== anonymous (no token) clone is refused =="
printf '#!/bin/sh\necho jjh_not_a_real_token\n' > "$TMP/askpass-bad.sh"
chmod +x "$TMP/askpass-bad.sh"
if SSH_ASKPASS="$TMP/askpass-bad.sh" git clone \
  "ssh://git@127.0.0.1:$SSH_PORT/${OWNER_NAME}.git" "$TMP/bad-clone" >/dev/null 2>&1; then
  echo "clone with wrong token unexpectedly succeeded"
  exit 1
fi

echo "== HTTP and SSH clones hold the same head =="
HTTP_HEAD=$(git -C "$TMP/ssh-clone" rev-parse HEAD)
jj git clone "$BASE/git/${OWNER_NAME}.git" "$TMP/http-clone" >/dev/null
HTTP_CLONE_HEAD=$(git -C "$TMP/http-clone" rev-parse HEAD)
test "$HTTP_HEAD" = "$HTTP_CLONE_HEAD" || {
  echo "heads differ: ssh=$HTTP_HEAD http=$HTTP_CLONE_HEAD"
  exit 1
}

echo "E2E SSH PASS"
