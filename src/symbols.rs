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
    Go,
    Rust,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Lang> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Some(Lang::JavaScript),
            Some("ts") | Some("mts") | Some("cts") => Some(Lang::TypeScript),
            Some("tsx") => Some(Lang::Tsx),
            Some("py") | Some("pyi") => Some(Lang::Python),
            Some("go") => Some(Lang::Go),
            Some("rs") => Some(Lang::Rust),
            _ => None,
        }
    }

    fn ts_language(self) -> tree_sitter::Language {
        match self {
            Lang::JavaScript => tree_sitter_javascript::language(),
            Lang::TypeScript => tree_sitter_typescript::language_typescript(),
            Lang::Tsx => tree_sitter_typescript::language_tsx(),
            Lang::Python => tree_sitter_python::language(),
            Lang::Go => tree_sitter_go::language(),
            Lang::Rust => tree_sitter_rust::language(),
        }
    }
}

/// Parse `source` and return the top-level symbols (functions + types + methods)
/// we lock on. We deliberately stay at file top level (plus type/impl members)
/// rather than recursing into every nested closure: the unit of a lock should be
/// a reviewable, nameable region.
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
    out.dedup_by(|a, b| a.name == b.name && a.start_line == b.start_line);
    Ok(out)
}

/// Names of node kinds that define a callable/type across our supported grammars.
fn classify(kind: &str) -> Option<&'static str> {
    match kind {
        // JS / TS / Go all use `function_declaration` for a top-level function.
        "function_declaration" | "generator_function_declaration" | "function_definition" => {
            Some("function")
        }
        // Rust free function.
        "function_item" => Some("function"),
        // Class-like types.
        "class_declaration" | "class_definition" => Some("class"),
        "struct_item" | "enum_item" | "trait_item" | "union_item" => Some("class"),
        // Methods.
        "method_definition" | "method_declaration" => Some("method"),
        // Rust trait method signatures (no body).
        "function_signature_item" => Some("method"),
        _ => None,
    }
}

fn collect_node(node: Node, src: &[u8], parent: Option<&str>, out: &mut Vec<Symbol>) {
    let kind = node.kind();

    // Unwrap export/decorated/visibility wrappers to reach the real declaration.
    if matches!(
        kind,
        "export_statement" | "decorated_definition" | "visibility_modifier"
    ) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_node(child, src, parent, out);
        }
        return;
    }

    // Go: `type_declaration` wraps one or more `type_spec` (the actual named type).
    if kind == "type_declaration" {
        let mut cursor = node.walk();
        for spec in node.named_children(&mut cursor) {
            if spec.kind() == "type_spec" {
                if let Some(name) = node_name(spec, src) {
                    out.push(symbol(name, "class", spec));
                }
            }
        }
        return;
    }

    // Rust: `impl_item` has no name; qualify its methods by the implemented type.
    if kind == "impl_item" {
        let type_name = node
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(src).ok())
            .unwrap_or("impl")
            .to_string();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for member in body.named_children(&mut cursor) {
                collect_node(member, src, Some(&type_name), out);
            }
        }
        return;
    }

    if let Some(sym_kind) = classify(kind) {
        if let Some(mut name) = node_name(node, src) {
            // Go methods carry a receiver; qualify with the receiver type so
            // `Type.Method` is unique and claimable.
            if kind == "method_declaration" {
                if let Some(recv) = go_receiver_type(node, src) {
                    name = format!("{recv}.{name}");
                }
            }
            let qualified = match parent {
                Some(p) if !name.contains('.') => format!("{p}.{name}"),
                _ => name.clone(),
            };
            // A callable nested inside a type/impl is a method, not a free function
            // (e.g. Rust `function_item` inside an `impl` block).
            let display_kind = if parent.is_some() && sym_kind == "function" {
                "method"
            } else {
                sym_kind
            };
            out.push(symbol(qualified.clone(), display_kind, node));

            // Descend into class/type bodies to capture members as their own symbols.
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

fn symbol(name: String, kind: &str, node: Node) -> Symbol {
    Symbol {
        name,
        kind: kind.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    }
}

/// Best-effort name extraction: most grammars expose a `name` field.
fn node_name(node: Node, src: &[u8]) -> Option<String> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("key"))?;
    name_node.utf8_text(src).ok().map(|s| s.to_string())
}

/// Extract the receiver type name of a Go method, e.g. `S` from `func (s S) M()`
/// or `func (s *S) M()`. Returns the first `type_identifier` under the receiver.
fn go_receiver_type(method: Node, src: &[u8]) -> Option<String> {
    let recv = method.child_by_field_name("receiver")?;
    first_kind(recv, "type_identifier").and_then(|n| n.utf8_text(src).ok().map(|s| s.to_string()))
}

/// Depth-first search for the first descendant node of the given kind.
fn first_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
        if let Some(found) = first_kind(child, kind) {
            return Some(found);
        }
    }
    None
}
