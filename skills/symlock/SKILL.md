---
name: symlock
description: >-
  Prevent parallel AI coding agents from colliding on the same code. Use this
  skill whenever multiple agents (or multiple sessions) may edit the same
  repository at the same time — before editing a function, class, or method,
  claim it with symlock so another agent working in a sibling worktree cannot
  edit the same region and cause a merge-time conflict. Trigger when: working in
  a git worktree that is one of several parallel agent workspaces; the user
  mentions running agents in parallel, fan-out, a squad/team of agents, or
  "don't step on the other agent"; or a claim/lock/conflict-prevention step is
  requested. Do NOT use for single-agent work on a repo no one else is touching.
---

# symlock — symbol-level conflict prevention

`symlock` lets parallel coding agents reserve the **symbol** (function / class /
method) they're about to edit, so two agents never modify the same region and
collide at merge time. It is a single Rust binary that reads/writes a shared
`.symlock/locks.json` at the repo root and uses cross-process file locking, so
claims from agents in different worktrees are race-free.

**Your job when this skill is active:** claim before you edit, respect
conflicts, release when done.

## Prerequisites (check once)

- Binary on PATH: run `symlock --version`. If missing, install it:
  `cargo install --git https://github.com/echoVic/symlock` (or grab a prebuilt
  binary from the repo's Releases page, or build from source with
  `cargo build --release` and use `target/release/symlock`).
- Store exists: `symlock status` should succeed. If it errors with "no .symlock
  directory", run `symlock init` at the repo root **once** (coordinate — only
  one agent needs to init).
- Identify yourself: set an agent id via `export SYMLOCK_AGENT=<your-id>` (e.g.
  your worktree/branch name). Then you can omit `--agent` on every call.
- **Working in a git worktree?** Worktrees are separate directory trees, so they
  won't find the main checkout's `.symlock`. Point every agent at one shared
  store: `export SYMLOCK_DIR=/path/to/main/repo/.symlock`. This is what makes
  claims visible across worktrees.

## The workflow — follow this order every time you edit code

1. **List symbols** in the file you intend to change:
   ```bash
   symlock symbols path/to/file.ts
   ```
   Use the exact symbol names it prints (methods are qualified, e.g.
   `TokenStore.issue`).

2. **Claim** each symbol you will edit, *before writing any code*:
   ```bash
   symlock claim path/to/file.ts login
   ```
   - **Exit 0** → the lock is yours. Proceed to edit.
   - **Exit 2** → CONFLICT. Another agent holds an overlapping region. **Do not
     edit it.** Read the reported holder, then either pick a different symbol,
     work on a different file, or tell the user these tasks overlap and should
     be serialized. Never wait-loop silently or force past a conflict.

3. **Edit** only the symbols you successfully claimed.

4. **Release** when the work on those symbols is done (e.g. before handing off
   or finishing the task):
   ```bash
   symlock release --symbol login path/to/file.ts   # one symbol
   symlock release                                   # all of my claims
   ```

## Rules

- **Always claim before editing** a function/class/method when this skill is
  active. An edit without a prior successful claim defeats the whole point.
- **A conflict is a stop sign, not a retry prompt.** On exit 2, change your plan
  — don't poll until it frees up unless the user asked you to.
- **Claim at the tightest scope.** Claim the specific method you'll touch, not
  the whole class, so other agents can work on sibling methods. (Claiming a
  class does lock all its methods — only do that if you're rewriting the class.)
- **Release promptly** so you don't block others longer than needed.
- **Unsupported files** (exit 1 "unsupported file type"): symlock only parses
  TS/JS/Python today. For other files, fall back to coordinating at file
  granularity with the user / orchestrator; do not assume it's safe.

## Machine-readable mode

Add `--json` to any command for structured output (useful when driving symlock
from a script or orchestrator). A conflict prints a `conflicts_with` array
naming the holding agent and the overlapping line range.

```bash
symlock --json claim path/to/file.ts login
```

## Quick reference

| command | purpose | exit codes |
|---|---|---|
| `symlock init` | create `.symlock/` at repo root (once) | 0 / 1 |
| `symlock symbols <file>` | list lockable symbols | 0 / 1 |
| `symlock claim [--agent id] <file> <symbol>` | reserve a symbol | 0 ok · 2 conflict · 1 error |
| `symlock release [--agent id] [--file f] [--symbol s]` | drop claims | 0 / 1 |
| `symlock status` | show all active claims | 0 / 1 |
