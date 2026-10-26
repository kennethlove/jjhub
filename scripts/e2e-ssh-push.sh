#!/usr/bin/env bash
# story: e17s02 — e2e: clone over ssh, push over ssh through the SAME
# admission chain (refused rewrite records push_rejections, refs untouched),
# and the re-push over ssh appends an MR revision (shared post-receive).
set -euo pipefail

cd "$(dirname "$0")/.."
PORT="${E2E_PORT:-3906}"
SSH_PORT="${E2E_SSH_PORT:-2202}"
BASE="http://127.0.0.1:${PORT}"
DB_URL="${DATABASE_URL:-postgres://postgres:jjhub@localhost:5432/jjhub_test}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"; kill "${SVPID:-0}" 2>/dev/null || true' EXIT

export SESSION_SECRET="${SESSION_SECRET:-e2e-shared-secret}"
cargo build -q -p server
SERVER_BIN="${CARGO_TARGET_DIR:-target}/debug/server"
JJHUB_STORAGE_ROOT="$TMP/repos" \
JJHUB_PORT="$PORT" \
JJHUB_SSH_ENABLED=1 \
JJHUB_SSH_BIND=127.0.0.1 \
JJHUB_SSH_PORT="$SSH_PORT" \
JJHUB_SSH_HOST_KEY_DIR="$TMP/ssh_host_keys" \
DATABASE_URL="$DB_URL" "$SERVER_BIN" &
SVPID=$!

# ${VAR:?why} at first use — the whole script depends on both listeners.
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

CJ="$TMP/cookies"
EMAIL="sshpush-e2e-$(date +%s%N)@example.com"

echo "== register + create repo =="
curl -sf -c "$CJ" -X POST "$BASE/register" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=correct horse battery" \
  --data-urlencode "password_confirm=correct horse battery" >/dev/null

SESSION=$(awk '$6 == "session" {print $7}' "$CJ")
CSRF=$(python3 -c "import hmac,hashlib,sys; print(hmac.new(b'e2e-shared-secret', sys.argv[1].encode(), hashlib.sha256).hexdigest())" "$SESSION")
LOC=$(curl -sf -b "$CJ" -o /dev/null -w '%{redirect_url}' -X POST "$BASE/repos" \
  -d "name=ssh-push-demo&visibility=public&_csrf=$CSRF")
OWNER_NAME="${LOC#"$BASE"/}"
HANDLE="${OWNER_NAME%%/*}"
BARE="$TMP/repos/${OWNER_NAME}.git"
echo "repo at $OWNER_NAME"

echo "== generate token =="
NEW_TOKEN=$(curl -sf -b "$CJ" -c "$CJ" -X POST "$BASE/settings/tokens" \
  -d "label=e2e&_csrf=$CSRF" -o /dev/null -D - | tr -d '\r' | awk -F'[=;]' '/set-cookie: new_token=/ {print $2}')
test -n "${NEW_TOKEN:?token issued}" || { echo "no token issued"; exit 1; }

AUTH_URL="http://x-access-token:$NEW_TOKEN@127.0.0.1:$PORT/git/${OWNER_NAME}.git"
SSH_URL="ssh://${HANDLE}@127.0.0.1:${SSH_PORT}/${OWNER_NAME}.git"

# password auth only: ssh must ask, and askpass answers with the token
cat > "$TMP/askpass.sh" <<'EOF'
#!/bin/sh
echo "${E2E_SSH_PASSWORD:?token for ssh password prompt}"
EOF
chmod +x "$TMP/askpass.sh"
export E2E_SSH_PASSWORD="$NEW_TOKEN"
export GIT_ASKPASS="$TMP/askpass.sh"
export SSH_ASKPASS="$TMP/askpass.sh"
export SSH_ASKPASS_REQUIRE=force
export GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=no -o PubkeyAuthentication=no -o PreferredAuthentications=password -o NumberOfPasswordPrompts=1"

echo "== jj git push fixture over http (opens the MR) =="
export JJ_USER="e2e" JJ_EMAIL="$EMAIL"
FIXTURE="$TMP/fixture"
jj git init --colocate "$FIXTURE" >/dev/null
(
  cd "$FIXTURE"
  echo "one" > a.txt
  jj describe -m "change one"
  jj bookmark create trunk 2>/dev/null || jj bookmark set trunk --allow-backwards 2>/dev/null || true
  jj git remote add origin "$AUTH_URL" >/dev/null 2>&1 || jj git remote set-url origin "$AUTH_URL"
  jj git push --all >/dev/null
)

sleep 1 # post-receive upsert is async to the CGI response
MR_ROWS=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM merge_requests mr
   JOIN repositories r ON r.id = mr.repo_id
   JOIN users u ON u.id = r.owner_id
   WHERE u.handle = split_part('$OWNER_NAME', '/', 1)
     AND r.name = split_part('$OWNER_NAME', '/', 2)
     AND mr.state = 'open'")
test "$MR_ROWS" -ge 1 || { echo "no open MR after push" >&2; exit 1; }
REVISIONS_BASE=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM revisions mrrev
   JOIN merge_requests mr ON mr.id = mrrev.mr_id
   JOIN repositories r ON r.id = mr.repo_id
   JOIN users u ON u.id = r.owner_id
   WHERE u.handle = split_part('$OWNER_NAME', '/', 1)")
test "$REVISIONS_BASE" -ge 1 || { echo "no MR revision after push" >&2; exit 1; }
echo "open MRs: $MR_ROWS, baseline revisions: $REVISIONS_BASE"

OLD_REV=$(git -C "$BARE" rev-parse refs/heads/trunk)

echo "== ssh clone with transport token =="
git clone "$SSH_URL" "$TMP/ssh-clone" >/dev/null
test -f "$TMP/ssh-clone/a.txt" || { echo "ssh clone missing a.txt"; exit 1; }

echo "== push a branch over ssh (same admission chain) =="
(
  cd "$TMP/ssh-clone"
  git switch -c feature >/dev/null 2>&1 || git checkout -b feature >/dev/null
  echo "feature work" > feature.txt
  git add feature.txt
  git -c user.name="e2e" -c user.email="$EMAIL" commit -m "feature commit" >/dev/null
  git push origin feature >/dev/null
)
git -C "$BARE" show-ref --verify --quiet refs/heads/feature \
  || { echo "feature branch missing in bare repo"; exit 1; }

echo "== foreign-author rewrite pushed over ssh is refused =="
(
  cd "$FIXTURE"
  jj git remote set-url origin "$SSH_URL"
  REV=$(jj log -r trunk --no-graph -T 'commit_id.short()')
  echo "two" > a.txt
  # jj-style foreign rewrite: describe -r keeps the change-id and stamps the
  # foreign committer; the colocated ref follows the rewrite
  JJ_USER="e2e" JJ_EMAIL="mallory@evil.example" \
    jj describe -r "$REV" --ignore-immutable -m "mallory rewrite" >/dev/null 2>&1
  # force the update PAST the client-side ff check so the server's
  # admission chain (fast-forward/identity rules) makes the decision
  if git -C "$FIXTURE" push --force --all >/dev/null 2>"$TMP/push.err"; then
    echo "foreign-author ssh push was ACCEPTED — invariant 2 broken" >&2; exit 1;
  fi
  grep -qi "owner.s identity" "$TMP/push.err" || {
    echo "rejection did not name the author-email rule:" >&2
    cat "$TMP/push.err" >&2; exit 1;
  }
)

echo "== push_rejections row recorded + ref untouched =="
REJ=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM push_rejections pr
   JOIN repositories r ON r.id = pr.repo_id
   JOIN users u ON u.id = r.owner_id
   WHERE u.handle = split_part('$OWNER_NAME', '/', 1)
     AND r.name = split_part('$OWNER_NAME', '/', 2)")
test "$REJ" -ge 1 || { echo "no push_rejections row after refused ssh push" >&2; exit 1; }
NEW_REV=$(git -C "$BARE" rev-parse refs/heads/trunk)
test "$OLD_REV" = "$NEW_REV" || {
  echo "trunk moved despite rejection: $OLD_REV -> $NEW_REV" >&2; exit 1;
}
echo "rejections: $REJ"

echo "== re-push by the real author over ssh appends a revision =="
(
  cd "$FIXTURE"
  # e10s01: protected trunk advances, never rewrites — the fixup is a child
  # of the accepted remote head (fast-forward) and still appends a revision
  jj new trunk@origin -m "author fixup"
  echo "three" > a.txt
  jj bookmark set trunk -r @ --allow-backwards >/dev/null
  git -C "$FIXTURE" push origin trunk >/dev/null
)
sleep 1 # post-receive effects run before exit-status, but stay paranoid
REVISIONS=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM revisions mrrev
   JOIN merge_requests mr ON mr.id = mrrev.mr_id
   JOIN repositories r ON r.id = mr.repo_id
   JOIN users u ON u.id = r.owner_id
   WHERE u.handle = split_part('$OWNER_NAME', '/', 1)")
test "$REVISIONS" -ge $((REVISIONS_BASE + 1)) || {
  echo "ssh re-push did not append a revision ($REVISIONS <= $REVISIONS_BASE)" >&2; exit 1;
}

echo "== ssh push to a fresh owned path creates the repo (e19s03) =="
git init -q -b trunk "$TMP/ssh-create"
(
  cd "$TMP/ssh-create"
  echo "created by ssh push" > new.txt
  git add new.txt
  git -c user.name="e2e" -c user.email="$EMAIL" commit -m "seed" >/dev/null
  git push "ssh://${HANDLE}@127.0.0.1:${SSH_PORT}/${HANDLE}/ssh-created.git" trunk >/dev/null
)
CREATE_VIS=$(psql "$DB_URL" -tAc \
  "SELECT visibility FROM repositories r JOIN users u ON u.id = r.owner_id \
   WHERE u.handle = '$HANDLE' AND r.name = 'ssh-created'")
test "$CREATE_VIS" = "private" || {
  echo "ssh push-created repo is '$CREATE_VIS', want private" >&2; exit 1; }
git -C "$TMP/repos/$HANDLE/ssh-created.git" show-ref --verify --quiet refs/heads/trunk || {
  echo "push-created bare repo has no trunk" >&2; exit 1; }
TIP=$(git -C "$TMP/repos/$HANDLE/ssh-created.git" log --oneline -1 refs/heads/trunk)
case "$TIP" in
  *seed*) ;;
  *) echo "created repo does not hold the seed commit: $TIP" >&2; exit 1;;
esac

echo "== clone of nothing over ssh stays refused, no row (e19s03) =="
if git ls-remote "ssh://${HANDLE}@127.0.0.1:${SSH_PORT}/${HANDLE}/never-ssh-created.git" \
    >/dev/null 2>&1; then
  echo "clone-of-nothing over ssh was accepted" >&2; exit 1
fi
NEVER=$(psql "$DB_URL" -tAc \
  "SELECT COUNT(*) FROM repositories r JOIN users u ON u.id = r.owner_id \
   WHERE u.handle = '$HANDLE' AND r.name = 'never-ssh-created'")
test "$NEVER" = "0" || { echo "upload-pack over ssh created a row" >&2; exit 1; }

echo "E2E SSH PUSH PASS"
