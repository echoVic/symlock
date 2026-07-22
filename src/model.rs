use serde::{Deserialize, Serialize};

/// A source symbol (top-level function, method, or class) with its line span.
/// Lines are 1-based and inclusive on both ends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Symbol {
    /// Fully-qualified-ish name, e.g. `calculateTotal` or `AuthService.login`.
    pub name: String,
    /// Symbol kind as reported by the language extractor: "function" | "class" | "method".
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
}

impl Symbol {
    /// Two symbols overlap if their line ranges intersect.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn overlaps(&self, other: &Symbol) -> bool {
        self.start_line <= other.end_line && other.start_line <= self.end_line
    }
}

/// An active claim: one agent reserving one symbol in one file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lock {
    pub agent: String,
    /// Repo-relative POSIX path.
    pub file: String,
    pub symbol: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Unix millis when the claim was created. Used for stale detection / audit.
    pub claimed_at_ms: u128,
}

/// On-disk shape of `.symlock/locks.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub locks: Vec<Lock>,
}

fn default_version() -> u32 {
    1
}
