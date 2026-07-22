# symlock

**Symbol-level locking & conflict prevention for parallel AI coding agents.**
Language-agnostic. Drop-in. Actually open source (Apache-2.0).

When you run N coding agents (Claude Code, Codex, Gemini CLI, …) in parallel git
worktrees, worktrees keep them from *overwriting* each other — but nothing stops
two agents from editing the **same function** and colliding at merge time, where
the fix isn't a one-line patch but a re-do of one agent's work.

`symlock` prevents that collision *before* it happens. Agents declare which
**symbol** (function / class / method) they're about to edit and acquire a
symbol-level lock. If another agent already holds an overlapping region, the
claim fails loudly with a structured warning — so work gets redirected before a
single wasted token.

```
$ symlock symbols auth.ts
function login          L1-3
function logout         L5-7
class    TokenStore     L9-16
method   TokenStore.issue   L10-12

$ symlock claim --agent codex  auth.ts login     # ok
$ symlock claim --agent claude auth.ts logout    # ok — different function
$ symlock claim --agent claude auth.ts login     # CONFLICT (exit 2)
CONFLICT: login (L1-3) overlaps an active claim:
  - login held by codex (L1-3)
```

## Why symlock (vs. the alternatives)

- **Actually open source** — Apache-2.0, no "source-available / free for ≤20 people" asterisk.
- **Language-agnostic** — symbol boundaries come from [tree-sitter](https://tree-sitter.github.io/), not regex. TS/JS + Python today; Go/Rust next.
- **Composable, not a walled garden** — a single binary + JSON output. Any orchestrator (Claude Squad, cmux, Vibe Kanban, or your own) can shell out to it or drive it over the planned MCP server. symlock is the missing *infrastructure*, not another dashboard.
- **Conservative by design** — it locks reviewable, nameable regions and warns on any overlap. It never silently merges anything (semantic AST merge is the next milestone, and will stay conservative: auto-merge only provably non-overlapping edits, everything else goes to a human).

## Install

```bash
cargo install --path .   # or: cargo build --release
```

## Usage

```bash
symlock init                                   # create .symlock/ in the repo
symlock symbols <file>                         # list lockable symbols
symlock claim  [--agent <id>] <file> <symbol>  # reserve a symbol (exit 2 on conflict)
symlock release [--agent <id>] [--file f] [--symbol s]
symlock status                                 # show all active claims
symlock --json <cmd>                           # machine-readable output for orchestrators
```

`--agent` can be omitted if you export `SYMLOCK_AGENT=<id>` (handy so each agent
sets its identity once per worktree).

### Use it from an AI coding agent (skill)

[`skill/SKILL.md`](skill/SKILL.md) is a drop-in [Agent Skill](https://agentskills.io/)
that teaches Claude Code / Codex / Cursor / OpenCode to **claim before they
edit** and back off on conflict. This is the intended way to wire symlock into a
parallel-agent workflow — no MCP handshake, no daemon, just a CLI on PATH plus
the behavior the skill injects. Install it into your agent's skills directory.

### Exit codes (part of the contract)

| code | meaning |
|------|---------|
| `0`  | success / claim granted |
| `2`  | conflict — an overlapping symbol is already claimed |
| `1`  | error (unsupported file, symbol not found, no `.symlock`, …) |

## Status

**MVP — conflict prevention.** Symbol extraction (TS/JS/Python), cross-process
safe claim/release, structured conflict reports, and an Agent Skill that makes
agents claim-before-edit. Roadmap: more languages (Go/Rust), and AST-level
semantic merge (conservative: auto-merge only provably non-overlapping edits).

## License

Apache-2.0.
