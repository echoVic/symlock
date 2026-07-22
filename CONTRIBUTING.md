# Contributing to symlock

Thanks for helping out. symlock is small and deliberately conservative — the
guiding rule is **correct or refuse, never silently wrong**, especially in
`merge`. Keep that in mind and most review comments write themselves.

## Development

```bash
cargo test                                   # unit tests (symbol extraction + merge)
cargo clippy --all-targets -- -D warnings    # lint (CI enforces this)
cargo fmt --all                              # format (CI checks --check)
./demo.sh                                    # end-to-end claim/conflict walkthrough
./tests/git-merge-driver.sh                  # real git merge driver integration
```

CI runs all of the above; run them locally before opening a PR.

## Adding a language

This is the highest-leverage contribution. Each language is a tree-sitter
grammar plus a little mapping. Steps:

1. Add the grammar crate in `Cargo.toml`, e.g. `tree-sitter-java = "0.21"`
   (match the `tree-sitter` 0.22-compatible line already used).
2. In `src/symbols.rs`:
   - add a variant to `enum Lang`;
   - map file extensions in `Lang::from_path`;
   - return the grammar from `Lang::ts_language`;
   - if the grammar names its declaration nodes differently, extend
     `classify()` (functions/classes/methods) and, for wrapper nodes like Go's
     `type_declaration` or Rust's `impl_item`, add handling in `collect_node`.
     The quickest way to learn the node kinds is a throwaway probe that walks
     `root.named_children()` and prints `node.kind()`.
3. Add a `extracts_<lang>_*` unit test in `src/main.rs` asserting the symbols
   (and their qualified names) you expect.
4. If the language is common, add a merge test too.

Symbol names must be **unique and stable** per file — methods are qualified by
their container (`Type.method`) so they don't collide.

## Touching `merge`

The merge path must stay provably safe. If you add an auto-merge case, it needs
a test proving it merges correctly **and** counter-example tests proving it
still refuses the unsafe neighbors. When in doubt, refuse (return a `Decline`).

## Commits & PRs

- Keep commits focused; describe the *why*.
- Update `CHANGELOG.md` under `## [Unreleased]`.
- All checks green before requesting review.
