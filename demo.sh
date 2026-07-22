#!/usr/bin/env bash
# symlock demo — two AI agents, two git worktrees, one shared repo.
#
# Shows the whole point of symlock in ~20 seconds:
#   - Two agents edit DIFFERENT functions in the same file  -> both proceed.
#   - A second agent tries the SAME function another holds   -> blocked (exit 2)
#     BEFORE any code is written, so no merge-time collision ever happens.
#
# Self-contained: builds symlock, creates a throwaway repo + worktrees in a
# temp dir, and cleans up on exit. Run from the symlock repo root: ./demo.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN="$ROOT/target/release/symlock"

cyan() { printf '\033[36m%s\033[0m\n' "$*"; }
dim()  { printf '\033[2m%s\033[0m\n' "$*"; }
run()  { printf '\033[32m$ %s\033[0m\n' "$*"; eval "$*" || true; echo; }

[ -x "$BIN" ] || { echo "building release binary..."; (cd "$ROOT" && cargo build --release -q); }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export PATH="$(dirname "$BIN"):$PATH"

# --- Set up a real git repo with a shared file ------------------------------
cyan "1. A repo with auth.ts, two agents about to work on it in parallel"
cd "$WORK"
git init -q repo && cd repo
git config user.email d@d.co && git config user.name d
cat > auth.ts <<'EOF'
export function login(user: string, pass: string): boolean {
  return check(user, pass);
}

export function logout(session: string): void {
  destroy(session);
}
EOF
git add -A && git commit -qm init

# One shared lock store; every worktree points at it via $SYMLOCK_DIR.
symlock init >/dev/null
export SYMLOCK_DIR="$WORK/repo/.symlock"

# Two worktrees = two isolated agent workspaces on the same repo.
git worktree add -q ../agentA -b agentA
git worktree add -q ../agentB -b agentB
dim "   worktrees: agentA/, agentB/  (sharing one .symlock store)"
echo

cyan "2. The lockable symbols symlock sees (via tree-sitter, not regex)"
run "symlock symbols '$WORK/repo/auth.ts'"

cyan "3. Agent A claims login; Agent B claims logout — different functions, both OK"
( cd "$WORK/agentA" && SYMLOCK_AGENT=agentA run "symlock claim auth.ts login" )
( cd "$WORK/agentB" && SYMLOCK_AGENT=agentB run "symlock claim auth.ts logout" )

cyan "4. Now Agent B ALSO wants login — which Agent A already holds. Blocked BEFORE editing:"
( cd "$WORK/agentB" && SYMLOCK_AGENT=agentB run "symlock claim auth.ts login; echo \"   -> exit \$?\"" )

cyan "5. Who holds what right now"
run "symlock status"

dim "That exit code 2 is the whole product: the collision was prevented up front,"
dim "instead of surfacing as an unmergeable conflict after both agents did the work."
