mod cli;
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
}
