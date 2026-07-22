//! Conservative semantic merge.
//!
//! Philosophy: **never produce a wrong merge.** Every automatic path here is
//! provably safe; anything we can't prove safe falls back to a conflict for a
//! human to resolve. Concretely:
//!
//! 1. Try a plain line-level 3-way merge (diffy). If it's clean, use it — a
//!    clean textual merge with no overlapping hunks is already correct.
//! 2. If diffy reports a conflict, fall back to symbol-aware analysis: if
//!    `ours` and `theirs` changed **disjoint** top-level symbols and neither
//!    touched anything outside those symbols (imports, layout, sibling code),
//!    the "conflict" was only textual adjacency. We can then rebuild the file
//!    by taking each side's version of the symbols it changed.
//! 3. Any doubt — different symbol sets, skeleton changes, added/removed/renamed
//!    symbols, duplicate names, parse failure, unsupported language — is a
//!    conflict. We do not guess.
//!
//! The symbol-aware result is re-parsed and its symbol set re-checked before we
//! trust it, as a final guard against a bad splice.

use std::collections::BTreeMap;
use std::path::Path;

use crate::symbols::{extract_symbols, Lang};

#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// A merge we are confident is correct.
    Clean(String),
    /// Could not prove safety. `text` is the diffy conflict-marked content
    /// (best-effort context for the human), `reason` explains why we declined
    /// to merge semantically.
    Conflict { text: String, reason: &'static str },
}

/// Reason a semantic merge was declined, for `--json` / diagnostics.
#[derive(Debug, Clone, Copy)]
pub enum Decline {
    SkeletonChanged,
    SymbolSetChanged,
    OverlappingSymbols,
    DuplicateSymbol,
    Unsupported,
    ParseFailed,
    ReverifyFailed,
    CarriageReturns,
}

impl Decline {
    pub fn as_str(self) -> &'static str {
        match self {
            Decline::SkeletonChanged => "skeleton (code outside symbols) changed on both sides",
            Decline::SymbolSetChanged => "symbols were added, removed, or renamed",
            Decline::OverlappingSymbols => "both sides changed the same symbol",
            Decline::DuplicateSymbol => "duplicate symbol names; cannot splice unambiguously",
            Decline::Unsupported => "unsupported language",
            Decline::ParseFailed => "parse failed",
            Decline::ReverifyFailed => "re-parsed result did not match expectations",
            Decline::CarriageReturns => {
                "CRLF line endings; semantic reconstruction is not byte-faithful"
            }
        }
    }
}

/// Merge `ours` and `theirs` against common ancestor `base`.
/// `path` is used only to pick the language.
pub fn merge(base: &str, ours: &str, theirs: &str, path: &Path) -> MergeOutcome {
    // Step 1: plain line-level 3-way merge.
    match diffy::merge(base, ours, theirs) {
        Ok(clean) => MergeOutcome::Clean(clean),
        Err(conflicted) => {
            // Step 2: try to prove the conflict is only textual adjacency.
            match semantic_merge(base, ours, theirs, path) {
                Ok(text) => MergeOutcome::Clean(text),
                Err(reason) => MergeOutcome::Conflict {
                    text: conflicted,
                    reason: reason.as_str(),
                },
            }
        }
    }
}

/// The provably-safe symbol partition merge. Returns `Err(Decline)` whenever we
/// cannot guarantee correctness.
fn semantic_merge(base: &str, ours: &str, theirs: &str, path: &Path) -> Result<String, Decline> {
    let lang = Lang::from_path(path).ok_or(Decline::Unsupported)?;

    // Our line-based reconstruction joins with '\n' and cannot faithfully
    // reproduce '\r\n' or bare '\r'. Rather than silently rewrite every line
    // ending in the file, refuse and let the line-level conflict stand.
    if [base, ours, theirs].iter().any(|s| s.contains('\r')) {
        return Err(Decline::CarriageReturns);
    }

    let base_map = symbol_map(lang, base)?;
    let ours_map = symbol_map(lang, ours)?;
    let theirs_map = symbol_map(lang, theirs)?;

    // The set of symbol names must be identical across all three. If a side
    // added/removed/renamed a symbol, positions shift in ways we won't reason
    // about — bail.
    if !same_key_set(&base_map, &ours_map) || !same_key_set(&base_map, &theirs_map) {
        return Err(Decline::SymbolSetChanged);
    }

    // The "skeleton" is everything *outside* symbol bodies (imports, top-level
    // statements, blank lines, layout). If both sides changed the skeleton we
    // can't safely combine; if only one side did, that's still risky because a
    // skeleton edit may depend on a symbol edit — require skeletons to be equal
    // after normalizing each side's own symbol regions out.
    let base_skel = skeleton(base, &base_map);
    let ours_skel = skeleton(ours, &ours_map);
    let theirs_skel = skeleton(theirs, &theirs_map);
    if ours_skel != base_skel || theirs_skel != base_skel {
        return Err(Decline::SkeletonChanged);
    }

    // Which symbols did each side actually change (body text differs from base)?
    let ours_changed = changed_symbols(&base_map, &ours_map);
    let theirs_changed = changed_symbols(&base_map, &theirs_map);

    // If both sides changed the same symbol, that's a real semantic conflict.
    if ours_changed.iter().any(|s| theirs_changed.contains(s)) {
        return Err(Decline::OverlappingSymbols);
    }

    // Build the merged file: start from base's symbol texts, then apply each
    // side's changes to its own disjoint set. Because the skeleton and symbol
    // set are identical, we can reconstruct deterministically from an ordered
    // template of (skeleton-gap, symbol) pieces taken from base.
    let merged = reconstruct(
        base,
        &base_map,
        &ours_map,
        &theirs_map,
        &ours_changed,
        &theirs_changed,
    );

    // Final guard: the result must parse and expose exactly the same symbol set.
    let merged_map = symbol_map(lang, &merged).map_err(|_| Decline::ReverifyFailed)?;
    if !same_key_set(&base_map, &merged_map) {
        return Err(Decline::ReverifyFailed);
    }

    Ok(merged)
}

/// Map of symbol name -> (start_line, end_line, body_text). Rejects duplicate
/// names, since we splice by name and duplicates would be ambiguous.
type SymMap = BTreeMap<String, (usize, usize, String)>;

fn symbol_map(lang: Lang, src: &str) -> Result<SymMap, Decline> {
    let syms = extract_symbols(lang, src).map_err(|_| Decline::ParseFailed)?;
    let lines: Vec<&str> = src.lines().collect();
    let mut map = SymMap::new();
    // Only consider *top-level* symbols (those not nested in another symbol's
    // range) to keep the partition non-overlapping. Nested methods are covered
    // by their container's range.
    let mut tops: Vec<&crate::model::Symbol> = Vec::new();
    for s in &syms {
        if !syms
            .iter()
            .any(|o| !std::ptr::eq(o, s) && o.start_line < s.start_line && o.end_line >= s.end_line)
        {
            tops.push(s);
        }
    }
    for s in tops {
        let body = lines
            .get(s.start_line - 1..s.end_line)
            .map(|l| l.join("\n"))
            .unwrap_or_default();
        if map
            .insert(s.name.clone(), (s.start_line, s.end_line, body))
            .is_some()
        {
            return Err(Decline::DuplicateSymbol);
        }
    }
    Ok(map)
}

fn same_key_set(a: &SymMap, b: &SymMap) -> bool {
    a.len() == b.len() && a.keys().all(|k| b.contains_key(k))
}

/// Symbols whose body text differs between `base` and `other`.
fn changed_symbols(base: &SymMap, other: &SymMap) -> Vec<String> {
    base.iter()
        .filter(|(k, (_, _, body))| other.get(*k).map(|(_, _, b)| b != body).unwrap_or(false))
        .map(|(k, _)| k.clone())
        .collect()
}

/// Everything outside symbol bodies, as a normalized string, so we can compare
/// whether the non-symbol scaffolding changed.
fn skeleton(src: &str, map: &SymMap) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut in_symbol = vec![false; lines.len() + 1];
    for (start, end, _) in map.values() {
        for slot in in_symbol
            .iter_mut()
            .take((*end).min(lines.len()) + 1)
            .skip((*start).max(1))
        {
            *slot = true;
        }
    }
    lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !in_symbol[*i + 1])
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rebuild the file line by line from `base`, substituting each changed
/// symbol's body from the side that changed it. Skeleton lines come from base
/// (guaranteed equal to both sides). Symbol bodies keep base unless a side
/// changed that symbol.
fn reconstruct(
    base: &str,
    base_map: &SymMap,
    ours_map: &SymMap,
    theirs_map: &SymMap,
    ours_changed: &[String],
    theirs_changed: &[String],
) -> String {
    let base_lines: Vec<&str> = base.lines().collect();

    // Order symbols by their start line in base so we emit them in place.
    let mut ordered: Vec<(&String, usize, usize)> =
        base_map.iter().map(|(k, (s, e, _))| (k, *s, *e)).collect();
    ordered.sort_by_key(|(_, s, _)| *s);

    let mut out: Vec<String> = Vec::new();
    let mut line_no = 1usize; // 1-based
    let mut idx = 0;
    while line_no <= base_lines.len() {
        if idx < ordered.len() && ordered[idx].1 == line_no {
            let (name, _s, e) = &ordered[idx];
            // Choose the body: whoever changed it (sets are disjoint), else base.
            let body = if ours_changed.contains(name) {
                &ours_map[*name].2
            } else if theirs_changed.contains(name) {
                &theirs_map[*name].2
            } else {
                &base_map[*name].2
            };
            out.push(body.clone());
            line_no = e + 1;
            idx += 1;
        } else {
            out.push(base_lines[line_no - 1].to_string());
            line_no += 1;
        }
    }
    let mut s = out.join("\n");
    // Preserve a trailing newline if base had one.
    if base.ends_with('\n') {
        s.push('\n');
    }
    s
}
