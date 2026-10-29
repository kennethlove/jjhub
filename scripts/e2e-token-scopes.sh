#!/usr/bin/env bash
# story: e22s02 — e2e: scope, pin, and IP allowlist enforced on BOTH
# transports: read-only clones but never pushes; a pinned token only
# touches its repo; an off-list client gets the invalid-token shape.
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3910}"
SSH_PORT="${E2E_SSH_PORT:-2206}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"; kill "${SVPID:-0}" "${WATCHPID:-0}" 2>/dev/null || true' EXIT

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
SERVER_SRC="${CARGO_TARGET_DIR:-target}/debug/server"
# WHY copy off target/: parallel sessions' pkill -f "…debug/server"
# cleanup patterns SIGKILL ours mid-run; a unique path stays matchless.
SERVER_BIN="$TMP/scope-e2e-srv"
cp "$SERVER_SRC" "$SERVER_BIN"
JJHUB_STORAGE_ROOT="$TMP/repos" \
JJHUB_PORT="$PORT" \
JJHUB_SSH_ENABLED=1 \
JJHUB_SSH_BIND=127.0.0.1 \
JJHUB_SSH_PORT="$SSH_PORT" \
JJHUB_SSH_HOST_KEY_DIR="$TMP/ssh_host_keys" \
DATABASE_URL="$DB_URL" "$SERVER_BIN" &
SVPID=$!

for _ in $(seq 1 50); do
  curl -sf "$BASE/healthz" >/dev/null && break
  sleep 0.1
done
curl -sf "$BASE/healthz" >/dev/null || { echo "http listener never came up"; exit 1; }

for _ in $(seq 1 50); do
  (exec 3<>"/dev/tcp/127.0.0.1/${SSH_PORT:?ssh port}") 2>/dev/null && break
  sleep 0.1
done
(exec 3<>"/dev/tcp/127.0.0.1/$SSH_PORT") 2>/dev/null || { echo "ssh listener never came up"; exit 1; }
( for i in $(seq 1 600); do
    hz=$(curl -s -S -m 2 -o /dev/null -w '%{http_code}' "$BASE/healthz" 2>/dev/null || true)
    cpu=$(ps -o %cpu= -p "$SVPID" 2>/dev/null | tr -d ' ' || true)
    echo "t=$i hz=$hz cpu=$cpu" >> "$TMP/watch.log"
    if [ "$hz" = "000" ]; then
      miss=$(( ${miss:-0} + 1 ))
      if [ "$miss" -eq 3 ] && [ ! -f "$TMP/spin.txt" ]; then
        ps -M "$SVPID" > "$TMP/spin-ps.txt" 2>&1 || true
        sample "$SVPID" 1 -file "$TMP/spin.txt" >/dev/null 2>&1 || true
      fi
    else
      miss=0
    fi
    sleep 1
  done ) &
WATCHPID=$!

CJ="$TMP/cookies"
EMAIL="tokenscope-e2e-$(date +%s%N)@example.com"

echo "== register + two private repos =="
curl -sf --max-time 20 -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" \
  --data-urlencode "password_confirm=correct horse battery" >/dev/null

SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")
LOC=$(curl -sf --max-time 20 -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d "name=alpha&visibility=private&_csrf=$CSRF")
ALPHA="${LOC#"$BASE"/}"
HANDLE="${ALPHA%%/*}"
curl -sf --max-time 20 -b "$CJ" -o /dev/null -X POST "$BASE/repos" \
  -d "name=beta&visibility=private&_csrf=$CSRF" || true
BETA="$HANDLE/beta"
BARE_ALPHA="$TMP/repos/${ALPHA}.git"
BARE_BETA="$TMP/repos/${BETA}.git"
echo "repos: $ALPHA, $BETA (private)"

echo "== seed alpha so pushes have somewhere to go =="
SEED="$TMP/seed"
git init -q -b trunk "$SEED"
git -C "$SEED" -c user.email=e2e@example.com -c user.name=e2e commit -q --allow-empty -m seed
git -C "$SEED" push -q "$BARE_ALPHA" trunk

echo "== mint three tokens =="
mint() {
  curl -sf --max-time 20 -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
    -d "label=$1&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' \
    | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}'
}
FULL=$(mint full)
RO=$(mint readonly)
PIN=$(mint pinned)
OFF=$(mint offlist)
test -n "${FULL:?full token}" && test -n "${RO:?ro token}" \
  && test -n "${PIN:?pin token}" && test -n "${OFF:?off token}"

echo "== apply restrictions (e22s02 columns) =="
psql "$DB_URL" -q -c \
  "UPDATE tokens SET scopes = '{read}'
   WHERE label = 'readonly'
     AND user_id = (SELECT id FROM users WHERE email = '$EMAIL')"
psql "$DB_URL" -q -c \
  "UPDATE tokens SET repo_id = (SELECT r.id FROM repositories r
     JOIN users u ON u.id = r.owner_id
    WHERE u.email = '$EMAIL' AND r.name = 'alpha')
   WHERE label = 'pinned'
     AND user_id = (SELECT id FROM users WHERE email = '$EMAIL')"
psql "$DB_URL" -q -c \
  "UPDATE tokens SET allowed_inets = '{203.0.113.9}'::inet[]
   WHERE label = 'offlist'
     AND user_id = (SELECT id FROM users WHERE email = '$EMAIL')"

cat > "$TMP/askpass.sh" <<'EOF'
#!/bin/sh
echo "${E2E_SSH_PASSWORD:?token for ssh password prompt}"
EOF
chmod +x "$TMP/askpass.sh"
export GIT_ASKPASS="$TMP/askpass.sh"
export SSH_ASKPASS="$TMP/askpass.sh"
export SSH_ASKPASS_REQUIRE=force
export GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=no -o PubkeyAuthentication=no -o PreferredAuthentications=password -o NumberOfPasswordPrompts=1"

# $1 token, $2 repo (owner/name), $3 workdir
git_over_http() { # token repo dir — args shift by 2 after env
  local token="$1" repo="$2" dir="$3"
  shift 3
  GIT_TERMINAL_PROMPT=0 git -c \
    "http.extraHeader=Authorization: Basic $(printf 'x-access-token:%s' "$token" | base64 | tr -d '\n')" \
    "$@" "http://127.0.0.1:$PORT/git/${repo}.git" "$dir" 2>"$TMP/git.err"
}
# same, exit code only captured: callers assert explicitly
run() { # cmd... — capture exit without tripping set -e
  set +e
  "$@" >"$TMP/run.out" 2>"$TMP/run.err"
  RC=$?
  set -e
}

echo "== read-only: clones everywhere, pushes nowhere =="
run git clone -q "http://x-access-token:$RO@127.0.0.1:$PORT/git/$ALPHA.git" "$TMP/ro-http"
test $RC -eq 0 || { echo "read-only HTTP clone failed" >&2; cat "$TMP/run.err"; exit 1; }

E2E_SSH_PASSWORD="$RO" run git clone -q "ssh://$HANDLE@127.0.0.1:$SSH_PORT/$ALPHA.git" "$TMP/ro-ssh"
test $RC -eq 0 || { echo "read-only SSH clone failed" >&2; cat "$TMP/run.err"; exit 1; }

run git -C "$TMP/ro-http" push -q "http://x-access-token:$RO@127.0.0.1:$PORT/git/$ALPHA.git" trunk
test $RC -ne 0 || { echo "read-only HTTP push must be refused"; exit 1; }
grep -qi "403\|forbidden" "$TMP/run.err" || { echo "expected a forbidden shape, got:"; cat "$TMP/run.err"; exit 1; }

cd "$TMP/ro-ssh"
run env E2E_SSH_PASSWORD="$RO" git push -q "ssh://$HANDLE@127.0.0.1:$SSH_PORT/$ALPHA.git" trunk
test $RC -ne 0 || { echo "read-only SSH push must be refused"; exit 1; }
grep -qi "forbidden\|exec request failed" "$TMP/run.err" || { echo "expected SSH refusal, got:"; cat "$TMP/run.err"; exit 1; }
cd - >/dev/null

echo "== pinned: alpha yes, beta no (both transports) =="
run git clone -q "http://x-access-token:$PIN@127.0.0.1:$PORT/git/$ALPHA.git" "$TMP/pin-alpha"
test $RC -eq 0 || { echo "pinned HTTP clone of its own repo failed"; cat "$TMP/run.err"; exit 1; }

CODE=$(run curl -s --max-time 20 -o /dev/null -w '%{http_code}' \
  -H "Authorization: Basic $(printf 'x-access-token:%s' "$PIN" | base64 | tr -d '\n')" \
  "$BASE/git/$BETA/info/refs?service=git-upload-pack")
OUT="$(cat "$TMP/run.out")"
if [ "$OUT" != "403" ]; then
  echo "pinned clone of beta expected 403, got: $OUT"
  echo "--- curl stderr:"; cat "$TMP/run.err" || true
  echo "--- server alive: $(kill -0 "$SVPID" 2>/dev/null && echo yes || echo NO)" || true
  echo "--- healthz: $(curl -s -S --max-time 3 -o /dev/null -w '%{http_code}' "$BASE/healthz" 2>&1 || true)"
  echo "--- watcher tail:"; tail -12 "$TMP/watch.log" 2>/dev/null || true
  echo "--- tokens rows: $(psql "$DB_URL" -tAc 'SELECT count(*) FROM tokens' 2>/dev/null || true)"
  cp "$TMP/spin.txt" /tmp/e22-spin.txt 2>/dev/null || echo "(no spin sample)"
  cp "$TMP/spin-ps.txt" /tmp/e22-spin-ps.txt 2>/dev/null || true
  echo "--- alive procs:"; ps -o pid,stat,etime,%cpu,time,command -p "$SVPID" 2>/dev/null || true
  sleep 2 || true
  echo "--- cpu after 2s:"; ps -o pid,%cpu,time -p "$SVPID" 2>/dev/null || true
  echo "--- server fds:"; lsof -p "$SVPID" 2>/dev/null | awk '{print $5}' | sort | uniq -c | sort -rn | head || true
  echo "--- listen 3910:"; netstat -an | grep -E "3910.*LISTEN" | head -3 || true
  echo "--- established 3910:"; netstat -an | grep "3910" | grep -v LISTEN | head -6 || true
  echo "--- git procs:"; ps aux | grep -E "[g]it-(remote-http|upload-pack)" | wc -l || true
  echo "--- sample:"; sample "$SVPID" 1 -file "$TMP/s.txt" 2>&1 | tail -1 || true
  grep -A6 "Call graph" "$TMP/s.txt" 2>/dev/null | head -20 || true
  grep -B2 -A8 "Thread_" "$TMP/s.txt" 2>/dev/null | grep -E "^[0-9]|server|upload|verify|loop|poll" | head -30 || true
  cp "$TMP/s.txt" /tmp/e22-sample.txt 2>/dev/null || true
  echo "--- per-thread cpu:"; ps -M "$SVPID" 2>/dev/null | head -12 || true
  exit 1
fi

run env E2E_SSH_PASSWORD="$PIN" git clone -q "ssh://$HANDLE@127.0.0.1:$SSH_PORT/$BETA.git" "$TMP/pin-beta"
test $RC -ne 0 || { echo "pinned SSH clone of beta must be refused"; exit 1; }
grep -qi "forbidden\|exec request failed" "$TMP/run.err" || { echo "expected SSH refusal for pin, got:"; cat "$TMP/run.err"; exit 1; }

echo "== off-list client: invalid-token shape, both transports =="
CODE=$(run curl -s --max-time 20 -o /dev/null -w '%{http_code}' \
  -H "Authorization: Basic $(printf 'x-access-token:%s' "$OFF" | base64 | tr -d '\n')" \
  "$BASE/git/$ALPHA/info/refs?service=git-upload-pack")
test "$(cat "$TMP/run.out")" = "401" || { echo "off-list HTTP expected 401, got: $(cat "$TMP/run.out")"; cat "$TMP/run.out"; exit 1; }

cd "$TMP/seed"
run env E2E_SSH_PASSWORD="$OFF" git push -q "ssh://$HANDLE@127.0.0.1:$SSH_PORT/$ALPHA.git" trunk
test $RC -ne 0 || { echo "off-list SSH push must be refused"; exit 1; }
grep -qi "denied\|permission\|access rights" "$TMP/run.err" || { echo "expected SSH auth rejection, got:"; cat "$TMP/run.err"; exit 1; }
cd - >/dev/null

echo "E2E TOKEN SCOPES PASS"
