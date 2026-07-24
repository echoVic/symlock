# Changelog

All notable changes to symlock are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions are [semver](https://semver.org/).

## [Unreleased]

## [0.4.0] — 2026-07-24
### Added
- **Wider semantic merge coverage**, all still provably safe:
  - A **one-sided skeleton change** (e.g. one side adds an import or a top-level
    line) now merges with the other side's disjoint symbol-body edits, instead
    of refusing.
  - A **symbol added on one side** now survives a merge with the other side's
    disjoint body edits, instead of refusing on "symbol set changed".
- New counter-example tests locking in the refusals that keep this safe: both
  sides changing the skeleton, both sides adding a symbol, and a structural side
  that also edits the same symbol the other side patched.

### Changed
- Merge internals reworked around a **"structure donor"** model: the structural
  side (or `base`) donates the file layout; the other side contributes only
  disjoint symbol-body edits, spliced by the donor's own line ranges. The result
  is still re-parsed and its symbol set re-verified before being trusted.

### Known limitations
- When **both** sides change the skeleton (e.g. each adds a *different* import),
  symlock still refuses and leaves a line-level conflict. A safe union-merge of
  additive-only skeleton changes is planned; until it can be proven correct,
  refusing is the honest default.

## [0.3.1] — 2026-07-22
### Fixed
- Semantic merge now **refuses CRLF files** instead of silently rewriting their
  line endings to LF — a merge must be correct or refuse, never silently wrong.
- `symlock merge` gained `--path` so it works as a **git merge driver**: git
  passes the working versions as extension-less temp files, which previously
  made every driver merge fall back to "unsupported language". Pass `%P` as
  `--path`; the README config is updated.
### Added
- Edge-case tests: absent trailing newline is preserved, empty files merge to
  empty, CRLF declines to a byte-preserving line-level conflict.
- `tests/git-merge-driver.sh` integration test (run in CI) proving the git merge
  driver auto-merges disjoint symbols and conflicts on same-symbol edits.
- `CHANGELOG.md`, `CONTRIBUTING.md`, and per-artifact `SHA256` checksums in
  release assets.

## [0.3.0] — 2026-07-22
### Added
- `symlock merge --base <b> --ours <o> --theirs <t> [-o out]`: conservative,
  symbol-aware 3-way merge. Cleanly combines edits to disjoint top-level symbols
  where plain `git` conflicts on adjacent lines.
- Usable as a git merge driver (see README).
### Safety
- Auto-merges only when both sides changed disjoint symbols and the skeleton
  (imports/layout/siblings) is unchanged; refuses (exit 2) on same-symbol edits,
  added/removed/renamed symbols, skeleton changes, duplicate names, parse
  failure, or unsupported language. Re-parses its own output before trusting it.

## [0.2.0] — 2026-07-22
### Added
- Go language support: functions, types (struct/interface), and
  receiver-qualified methods (`Server.Start`).
- Rust language support: functions, structs/enums/traits, and `impl` methods
  qualified by type (`Cache.get`).

## [0.1.0] — 2026-07-22
### Added
- Initial release: symbol-level conflict prevention.
- `init` / `symbols` / `claim` / `release` / `status` with an exit-code contract
  (0 ok, 2 conflict, 1 error) and `--json` output.
- tree-sitter symbol extraction for TypeScript/JavaScript and Python.
- Cross-process safe claim/release via a shared `.symlock/locks.json`.
- Agent Skill (`skills/symlock/SKILL.md`) that makes coding agents claim before
  they edit; installable via `npx skills add echoVic/symlock`.

[Unreleased]: https://github.com/echoVic/symlock/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/echoVic/symlock/releases/tag/v0.4.0
[0.3.1]: https://github.com/echoVic/symlock/releases/tag/v0.3.1
[0.3.0]: https://github.com/echoVic/symlock/releases/tag/v0.3.0
[0.2.0]: https://github.com/echoVic/symlock/releases/tag/v0.2.0
[0.1.0]: https://github.com/echoVic/symlock/releases/tag/v0.1.0
