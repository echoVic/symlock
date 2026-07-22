mod cli;
mod merge;
mod model;
mod store;
mod symbols;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    cli::run(cli)
}

#[cfg(test)]
mod tests {
    use crate::symbols::{extract_symbols, Lang};

    #[test]
    fn extracts_top_level_js_functions() {
        let src = "function a() {\n  return 1;\n}\n\nconst x = 2;\n\nfunction b() {}\n";
        let syms = extract_symbols(Lang::JavaScript, src).unwrap();
        let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn extracts_class_methods_as_qualified_symbols() {
        let src = "class Auth {\n  login() {}\n  logout() {}\n}\n";
        let syms = extract_symbols(Lang::JavaScript, src).unwrap();
        let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Auth"));
        assert!(names.contains(&"Auth.login"));
        assert!(names.contains(&"Auth.logout"));
    }

    #[test]
    fn extracts_exported_typescript_functions() {
        let src = "export function foo(x: number): number {\n  return x;\n}\n";
        let syms = extract_symbols(Lang::TypeScript, src).unwrap();
        assert!(syms.iter().any(|s| s.name == "foo"));
    }

    #[test]
    fn extracts_python_functions_and_classes() {
        let src = "def a():\n    pass\n\nclass C:\n    def m(self):\n        pass\n";
        let syms = extract_symbols(Lang::Python, src).unwrap();
        let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"C"));
        assert!(names.contains(&"C.m"));
    }

    #[test]
    fn extracts_go_functions_types_and_methods() {
        let src = "package main\n\nfunc Add(a, b int) int {\n\treturn a + b\n}\n\n\
                   type Server struct{}\n\nfunc (s *Server) Start() {}\n\nfunc (s Server) Stop() {}\n";
        let syms = extract_symbols(Lang::Go, src).unwrap();
        let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Add"), "got {names:?}");
        assert!(names.contains(&"Server"), "got {names:?}");
        // Methods qualified by receiver type (pointer and value receivers alike).
        assert!(names.contains(&"Server.Start"), "got {names:?}");
        assert!(names.contains(&"Server.Stop"), "got {names:?}");
    }

    #[test]
    fn extracts_rust_functions_structs_and_impl_methods() {
        let src = "pub fn add(a: i32) -> i32 {\n    a\n}\n\nstruct S;\n\n\
                   impl S {\n    fn m(&self) {}\n    pub fn n(&self) {}\n}\n\ntrait T {\n    fn f(&self);\n}\n";
        let syms = extract_symbols(Lang::Rust, src).unwrap();
        let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"add"), "got {names:?}");
        assert!(names.contains(&"S"), "got {names:?}");
        assert!(names.contains(&"S.m"), "got {names:?}");
        assert!(names.contains(&"S.n"), "got {names:?}");
        assert!(names.contains(&"T"), "got {names:?}");
    }

    #[test]
    fn overlap_detection_is_symmetric_and_correct() {
        use crate::model::Symbol;
        let s = |a, b| Symbol {
            name: "x".into(),
            kind: "function".into(),
            start_line: a,
            end_line: b,
        };
        assert!(s(1, 5).overlaps(&s(5, 9))); // touch at boundary
        assert!(s(1, 10).overlaps(&s(3, 4))); // contained
        assert!(!s(1, 5).overlaps(&s(6, 9))); // disjoint
    }

    // ---- semantic merge ----
    use crate::merge::{merge, MergeOutcome};
    use std::path::Path;

    fn ts(name: &str) -> std::path::PathBuf {
        Path::new(name).to_path_buf()
    }

    fn assert_clean(o: MergeOutcome) -> String {
        match o {
            MergeOutcome::Clean(s) => s,
            MergeOutcome::Conflict { reason, .. } => {
                panic!("expected clean merge, got conflict: {reason}")
            }
        }
    }

    fn assert_conflict(o: MergeOutcome) {
        assert!(
            matches!(o, MergeOutcome::Conflict { .. }),
            "expected conflict, got clean merge"
        );
    }

    #[test]
    fn merges_adjacent_edits_to_different_functions() {
        // Adjacent single-line funcs => diffy conflicts => the semantic layer
        // must recognize the edits hit disjoint symbols and combine them.
        let base = "function a(){return 1;}\nfunction b(){return 2;}\n";
        let ours = "function a(){return 111;}\nfunction b(){return 2;}\n";
        let theirs = "function a(){return 1;}\nfunction b(){return 222;}\n";
        // sanity: this input really does defeat plain diffy
        assert!(diffy::merge(base, ours, theirs).is_err());
        let out = assert_clean(merge(base, ours, theirs, &ts("a.js")));
        assert!(out.contains("111"), "got: {out}");
        assert!(out.contains("222"), "got: {out}");
    }

    #[test]
    fn conflicts_when_both_edit_the_same_function() {
        let base = "function f() {\n  return 1;\n}\n";
        let ours = "function f() {\n  return 2;\n}\n";
        let theirs = "function f() {\n  return 3;\n}\n";
        assert_conflict(merge(base, ours, theirs, &ts("a.js")));
    }

    #[test]
    fn conflicts_when_a_symbol_is_added_amid_adjacent_edits() {
        // Adjacent single-line funcs force a diffy conflict → semantic layer runs;
        // theirs also ADDS a function, so the symbol set differs → refuse to guess.
        let base = "function a(){return 1;}\nfunction b(){return 2;}\n";
        let ours = "function a(){return 111;}\nfunction b(){return 2;}\n";
        let theirs =
            "function a(){return 1;}\nfunction b(){return 222;}\nfunction c(){return 3;}\n";
        assert_conflict(merge(base, ours, theirs, &ts("a.js")));
    }

    #[test]
    fn non_adjacent_symbol_add_merges_cleanly_via_diffy() {
        // A safe, non-overlapping symbol addition IS correct to auto-merge —
        // diffy handles it and we trust that.
        let base = "function a() {\n  return 1;\n}\n";
        let ours = "function a() {\n  return 9;\n}\n";
        let theirs = "function a() {\n  return 1;\n}\n\nfunction b() {\n  return 2;\n}\n";
        let out = assert_clean(merge(base, ours, theirs, &ts("a.js")));
        assert!(
            out.contains("return 9") && out.contains("function b"),
            "got: {out}"
        );
    }

    #[test]
    fn conflicts_when_skeleton_changes_on_both_sides() {
        // Both sides touch the top-level import line (outside any symbol).
        let base = "import x from 'a';\nfunction f() {\n  return 1;\n}\n";
        let ours = "import x from 'b';\nfunction f() {\n  return 1;\n}\n";
        let theirs = "import x from 'c';\nfunction f() {\n  return 1;\n}\n";
        assert_conflict(merge(base, ours, theirs, &ts("a.js")));
    }

    #[test]
    fn clean_when_diffy_already_handles_it() {
        // Non-adjacent edits diffy merges without help.
        let base = "function a() {\n  return 1;\n}\n\n\n\nfunction b() {\n  return 2;\n}\n";
        let ours = "function a() {\n  return 9;\n}\n\n\n\nfunction b() {\n  return 2;\n}\n";
        let theirs = "function a() {\n  return 1;\n}\n\n\n\nfunction b() {\n  return 8;\n}\n";
        let out = assert_clean(merge(base, ours, theirs, &ts("a.js")));
        assert!(out.contains("return 9") && out.contains("return 8"));
    }

    #[test]
    fn merges_disjoint_go_methods() {
        let base = "package m\nfunc (s *S) A() {\n\treturn\n}\nfunc (s *S) B() {\n\treturn\n}\n";
        let ours = "package m\nfunc (s *S) A() {\n\tx()\n}\nfunc (s *S) B() {\n\treturn\n}\n";
        let theirs = "package m\nfunc (s *S) A() {\n\treturn\n}\nfunc (s *S) B() {\n\ty()\n}\n";
        let out = assert_clean(merge(base, ours, theirs, &ts("s.go")));
        assert!(out.contains("x()") && out.contains("y()"), "got: {out}");
    }

    // ---- edge cases: a merge tool must be correct-or-refuse, never silently wrong ----

    #[test]
    fn preserves_absence_of_trailing_newline() {
        let base = "function a(){return 1;}\nfunction b(){return 2;}";
        let ours = "function a(){return 111;}\nfunction b(){return 2;}";
        let theirs = "function a(){return 1;}\nfunction b(){return 222;}";
        let out = assert_clean(merge(base, ours, theirs, &ts("x.js")));
        assert!(
            !out.ends_with('\n'),
            "must not invent a trailing newline: {out:?}"
        );
        assert!(out.contains("111") && out.contains("222"));
    }

    #[test]
    fn refuses_crlf_rather_than_rewrite_line_endings() {
        // Semantic reconstruction joins with '\n'; silently converting a whole
        // CRLF file to LF would be a wrong merge. Refuse instead.
        let base = "function a(){return 1;}\r\nfunction b(){return 2;}\r\n";
        let ours = "function a(){return 111;}\r\nfunction b(){return 2;}\r\n";
        let theirs = "function a(){return 1;}\r\nfunction b(){return 222;}\r\n";
        match merge(base, ours, theirs, &ts("x.js")) {
            MergeOutcome::Conflict { text, .. } => {
                // Falls back to the byte-preserving line-level conflict.
                assert!(text.contains('\r'), "conflict text should keep CRLF bytes");
            }
            MergeOutcome::Clean(s) => panic!("CRLF must not auto-merge; got {s:?}"),
        }
    }

    #[test]
    fn empty_files_merge_to_empty() {
        assert_eq!(assert_clean(merge("", "", "", &ts("x.js"))), "");
    }
}
