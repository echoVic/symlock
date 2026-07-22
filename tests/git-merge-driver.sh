#!/usr/bin/env bash
# Integration test: symlock as a real git merge driver.
#
# Proves the workflow the README advertises actually works end-to-end:
#   - disjoint-symbol edits that git alone conflicts on are auto-merged
#   - same-symbol edits still produce a normal git conflict for a human
#
# Run from the repo root: ./tests/git-merge-driver.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/symlock"
[ -x "$BIN" ] || (cd "$ROOT" && cargo build --release -q)

fail() { echo "FAIL: $1" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

git init -q
git config user.email t@t.co && git config user.name t
git config merge.symlock.name "symlock semantic merge"
git config merge.symlock.driver "$BIN merge --base %O --ours %A --theirs %B -o %A --path %P"
echo "*.js merge=symlock" > .gitattributes
git add .gitattributes && git commit -qm setup

printf 'function a(){return 1;}\nfunction b(){return 2;}\n' > f.js
git add f.js && git commit -qm base

# Case 1: disjoint symbols -> auto-merged.
git checkout -q -b feat
printf 'function a(){return 1;}\nfunction b(){return 222;}\n' > f.js
git commit -qam theirs
git checkout -q master
printf 'function a(){return 111;}\nfunction b(){return 2;}\n' > f.js
git commit -qam ours

if ! git merge feat -m merge >/dev/null 2>&1; then
  fail "disjoint-symbol merge should have succeeded"
fi
grep -q 111 f.js && grep -q 222 f.js || fail "merged file missing both changes"
grep -q '<<<<<<<' f.js && fail "merged file has conflict markers"
echo "ok: disjoint symbols auto-merged"

# Case 2: both edit the same function -> real conflict left for a human.
git checkout -q -b feat2
printf 'function a(){return 555;}\nfunction b(){return 2;}\n' > f.js
git commit -qam theirs2
git checkout -q master
printf 'function a(){return 999;}\nfunction b(){return 2;}\n' > f.js
git commit -qam ours2

if git merge feat2 -m merge2 >/dev/null 2>&1; then
  fail "same-symbol merge should have conflicted, not auto-merged"
fi
grep -q '<<<<<<<' f.js || fail "expected conflict markers for same-symbol edit"
echo "ok: same-symbol edit left a conflict for a human"

echo "PASS: git merge driver integration"
