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
JJHUB_STORAGE_ROOT="$TMP/repos" JJHUB_PORT="$PORT" DATABASE_URL="$DB_URL" JJHUB_EMAIL_CONFIRM=off ./target/debug/server &
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

# story: e08s01 — build jj's git conflict representation into the same bare
# repo with plain git plumbing (no jj CLI needed), then clone the conflicted
# branch through the running server and prove the objects survive.
echo "== conflicted fixture clone =="
BARE="$TMP/repos/${OWNER_NAME:?repo redirect}.git"
# plumbing only: never let user git config touch the fixture
fg() { GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null git -C "$BARE" "$@"; }
blob() { printf '%s\n' "$1" | fg hash-object -t blob -w --stdin; }
term_tree() { printf '100644 blob %s\tconflict.txt\n' "$1" | fg mktree; }

base_blob=$(blob base)
side0_blob=$(blob side0)
side1_blob=$(blob side1)
base_tree=$(term_tree "$base_blob")
side0_tree=$(term_tree "$side0_blob")
side1_tree=$(term_tree "$side1_blob")
readme=$(fg hash-object -t blob -w --stdin <<'EOF'
This commit was made by jj, https://jj-vcs.dev/.
The commit contains file conflicts, and therefore looks wrong when used with
plain Git or other tools that are unfamiliar with jj.

The .jjconflict-* directories represent the different inputs to the conflict.
For details, see
https://docs.jj-vcs.dev/latest/git-compatibility/#format-mapping-details

If you see this file in your working copy, it probably means that you used a
regular `git` command to check out a conflicted commit. Use `jj abandon` to
recover.
EOF
)
conflict_tree=$(printf '040000 tree %s\t.jjconflict-base-0\n040000 tree %s\t.jjconflict-side-0\n040000 tree %s\t.jjconflict-side-1\n100644 blob %s\tJJ-CONFLICT-README\n100644 blob %s\tconflict.txt\n' \
  "$base_tree" "$side0_tree" "$side1_tree" "$readme" "$base_blob" | fg mktree)
sig="e2e <e2e@example.com> 1700000000 +0000"
parent=$(printf 'tree %s\nauthor %s\ncommitter %s\nchange-id 9a8b7c6d5e4f30211203f4e5d6c7b8a9\n\ne08s01 parent commit\n' \
  "$base_tree" "$sig" "$sig" | fg hash-object -t commit -w --stdin)
conflicted=$(printf 'tree %s\nparent %s\nauthor %s\ncommitter %s\njj:trees %s %s %s\nchange-id 0e1f2d3c4b5a69788796a5b4c3d2e1f0\n\ne08s01 conflicted commit\n' \
  "$conflict_tree" "$parent" "$sig" "$sig" "$base_tree" "$side0_tree" "$side1_tree" \
  | fg hash-object -t commit -w --stdin)
fg update-ref refs/heads/conflicted "$conflicted"
fg update-ref "refs/jj/keep/$parent" "$parent"
fg update-ref "refs/jj/keep/$conflicted" "$conflicted"

echo "== ls-remote advertises refs/jj/keep/* =="
git ls-remote "$BASE/git/${OWNER_NAME}.git" | grep -q "refs/jj/keep/$conflicted" \
  || { echo "keep ref not advertised"; exit 1; }

echo "== clone the conflicted branch and fsck =="
git clone --branch conflicted "$BASE/git/${OWNER_NAME}.git" "$TMP/conflict-clone" >/dev/null
git -C "$TMP/conflict-clone" fsck
git -C "$TMP/conflict-clone" cat-file commit HEAD | grep -q '^jj:trees '
git -C "$TMP/conflict-clone" ls-tree HEAD | grep -q '.jjconflict-base-0'
git -C "$TMP/conflict-clone" fetch origin '+refs/jj/keep/*:refs/jj/keep/*' >/dev/null
git -C "$TMP/conflict-clone" rev-parse "refs/jj/keep/$conflicted" | grep -q "$conflicted"

echo "E2E PASS"
