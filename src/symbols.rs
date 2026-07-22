use anyhow::{anyhow, Context, Result};
use std::path::Path;
use tree_sitter::{Node, Parser};

use crate::model::Symbol;

/// Supported languages, resolved from a file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    JavaScript,
    TypeScript,
    Tsx,
    Python,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Lang> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Some(Lang::JavaScript),
            Some("ts") | Some("mts") | Some("cts") => Some(Lang::TypeScript),
            Some("tsx") => Some(Lang::Tsx),
            Some("py") | Some("pyi") => Some(Lang::Python),
            _ => None,
        }
    }

    fn ts_language(self) -> tree_sitter::Language {
        match self {
            Lang::JavaScript => tree_sitter_javascript::language(),
            Lang::TypeScript => tree_sitter_typescript::language_typescript(),
            Lang::Tsx => tree_sitter_typescript::language_tsx(),
            Lang::Python => tree_sitter_python::language(),
        }
    }
}

/// Parse `source` and return the top-level symbols (functions + classes) we lock on.
/// We deliberately stay at file top level (and class members) rather than recursing
/// into every nested closure: the unit of a lock should be a reviewable, nameable region.
pub fn extract_symbols(lang: Lang, source: &str) -> Result<Vec<Symbol>> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .context("failed to set tree-sitter language")?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("tree-sitter failed to parse source"))?;

    let root = tree.root_node();
    let mut out = Vec::new();
    let bytes = source.as_bytes();

    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        collect_node(child, bytes, None, &mut out);
    }
    out.sort_by_key(|s| s.start_line);
    Ok(out)
}

/// Names of node kinds that define a callable/class across our supported grammars.
fn classify(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration" | "generator_function_declaration" | "function_definition" => {
            Some("function")
        }
        "class_declaration" | "class_definition" => Some("class"),
        "method_definition" => Some("method"),
        // `export function foo()` / `export default class` wrap the real decl.
        _ => None,
    }
}

fn collect_node(node: Node, src: &[u8], parent: Option<&str>, out: &mut Vec<Symbol>) {
    let kind = node.kind();

    // Unwrap export/decorated wrappers to reach the underlying declaration.
    if kind == "export_statement" || kind == "decorated_definition" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_node(child, src, parent, out);
        }
        return;
    }

    if let Some(sym_kind) = classify(kind) {
        if let Some(name) = node_name(node, src) {
            let qualified = match parent {
                Some(p) => format!("{p}.{name}"),
                None => name.clone(),
            };
            out.push(Symbol {
                name: qualified.clone(),
                kind: sym_kind.to_string(),
                start_line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
            });

            // For classes, descend one level to capture methods as their own symbols.
            if sym_kind == "class" {
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for member in body.named_children(&mut cursor) {
                        collect_node(member, src, Some(&qualified), out);
                    }
                }
            }
        }
    }
}

/// Best-effort name extraction: most grammars expose a `name` field.
fn node_name(node: Node, src: &[u8]) -> Option<String> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("key"))?;
    name_node.utf8_text(src).ok().map(|s| s.to_string())
}
