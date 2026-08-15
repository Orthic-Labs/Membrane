//! L3 code skeletonizer — extract signatures/decls from source files.
//!
//! v1 supports a small set of common languages (Rust/Python/JS/TS). Unsupported
//! extensions pass through unchanged (parity with the workspace `skel.py`
//! fallback).

use std::path::Path;

use tree_sitter::{Language, Node, Parser, TreeCursor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetSkeleton {
    pub text: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub budget_tokens: usize,
    pub level: &'static str,
    pub budget_met: bool,
    /// Present only when full source bytes were durably published to a
    /// resolvable recovery handle. Pure skeletonization does not publish.
    pub recovery_marker: Option<crate::compress::RecoveryMarkerV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
}

fn lang_for_path(path: &Path) -> Option<Lang> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some(Lang::Rust),
        "py" => Some(Lang::Python),
        "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
        "ts" | "tsx" => Some(Lang::TypeScript),
        _ => None,
    }
}

fn ts_language(lang: Lang) -> Language {
    match lang {
        Lang::Rust => tree_sitter_rust::language(),
        Lang::Python => tree_sitter_python::language(),
        Lang::JavaScript => unsafe {
            std::mem::transmute::<*const (), tree_sitter::Language>(
                (tree_sitter_javascript::LANGUAGE.into_raw())(),
            )
        },
        Lang::TypeScript => tree_sitter_typescript::language_typescript(),
    }
}

fn render_rust_item(src: &str, node: Node) -> Option<String> {
    let body = node
        .child_by_field_name("body")
        .or_else(|| {
            node.children(&mut node.walk())
                .find(|c| c.kind() == "block")
        })
        .or_else(|| {
            node.children(&mut node.walk())
                .find(|c| c.kind() == "declaration_list")
        })
        .or_else(|| {
            node.children(&mut node.walk())
                .find(|c| c.kind() == "field_declaration_list")
        });
    let snippet = match body {
        Some(body) => {
            let start = node.start_byte();
            let body_start = body.start_byte();
            let prefix = src.get(start..body_start).unwrap_or("").trim_end();
            format!("{prefix}{{ /* body elided */ }}")
        }
        None => node.utf8_text(src.as_bytes()).ok()?.trim().to_string(),
    };
    if snippet.is_empty() {
        None
    } else {
        Some(snippet)
    }
}

fn render_python_def(src: &str, node: Node) -> Option<String> {
    let text = node.utf8_text(src.as_bytes()).ok()?;
    let header = text.lines().next().unwrap_or("").trim_end();
    if header.is_empty() {
        None
    } else if header.ends_with(':') {
        Some(format!("{header}\n    pass  # body elided"))
    } else {
        Some(format!("{header}:\n    pass  # body elided"))
    }
}

fn render_js_like(src: &str, node: Node) -> Option<String> {
    let text = node.utf8_text(src.as_bytes()).ok()?;
    let first = text.lines().next().unwrap_or("").trim_end();
    if first.is_empty() {
        None
    } else if first.contains('{') {
        // Keep only the signature line, replace body.
        let sig = first.split('{').next().unwrap_or(first).trim_end();
        Some(format!("{sig}{{ /* body elided */ }}"))
    } else {
        Some(first.to_string())
    }
}

fn collect_top_level<'a>(cursor: &mut TreeCursor<'a>, out: &mut Vec<Node<'a>>) {
    // Simple DFS collecting only top-level items; we don't try to skeletonize nested definitions yet.
    if (cursor.node().kind() == "source_file"
        || cursor.node().kind() == "module"
        || cursor.node().kind() == "program")
        && cursor.goto_first_child()
    {
        loop {
            out.push(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Skeletonize source code based on the file extension.
///
/// Unsupported extensions return `src` unchanged.
pub fn skeletonize(path: &Path, src: &str) -> String {
    let Some(lang) = lang_for_path(path) else {
        return src.to_string();
    };

    let mut parser = Parser::new();
    if parser.set_language(&ts_language(lang)).is_err() {
        return src.to_string();
    }
    let Some(tree) = parser.parse(src, None) else {
        return src.to_string();
    };

    let mut nodes = Vec::new();
    collect_top_level(&mut tree.walk(), &mut nodes);

    let mut out = Vec::new();
    for n in nodes {
        let kind = n.kind();
        let item = match lang {
            Lang::Rust => match kind {
                "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item"
                | "mod_item" | "type_item" | "const_item" | "static_item" | "use_declaration" => {
                    render_rust_item(src, n)
                }
                _ => None,
            },
            Lang::Python => match kind {
                "function_definition" | "class_definition" => render_python_def(src, n),
                _ => None,
            },
            Lang::JavaScript | Lang::TypeScript => match kind {
                "function_declaration" | "class_declaration" | "lexical_declaration" => {
                    render_js_like(src, n)
                }
                _ => None,
            },
        };
        if let Some(s) = item {
            out.push(s);
        }
    }

    if out.is_empty() {
        // Parity with `skel.py`'s fallback: emit original if we couldn't skeletonize meaningfully.
        src.to_string()
    } else {
        out.join("\n")
    }
}

/// Skeletonize within a token budget, degrading predictably:
/// signature skeleton, public signatures, then a recoverable path stub.
pub fn skeletonize_to_budget(path: &Path, src: &str, budget_tokens: usize) -> BudgetSkeleton {
    let input_tokens = crate::compress::estimate_tokens(src);
    let signature = skeletonize(path, src);
    if crate::compress::estimate_tokens(&signature) <= budget_tokens {
        return budget_result(signature, src, path, input_tokens, budget_tokens, "signature");
    }

    let public = signature
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("pub ")
                || trimmed.starts_with("pub(")
                || trimmed.starts_with("export ")
                || trimmed.starts_with("public ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !public.is_empty() && crate::compress::estimate_tokens(&public) <= budget_tokens {
        return budget_result(public, src, path, input_tokens, budget_tokens, "public-signature");
    }

    let comment = if matches!(
        path.extension().and_then(|x| x.to_str()),
        Some("py") | Some("sh") | Some("bash")
    ) {
        '#'
    } else {
        '/'
    };
    let stub = if comment == '#' {
        format!("# crypt: {} elided; retrieve original", path.display())
    } else {
        format!("// crypt: {} elided; retrieve original", path.display())
    };
    budget_result(stub, src, path, input_tokens, budget_tokens, "path-stub")
}

fn budget_result(
    text: String,
    _source: &str,
    _path: &Path,
    input_tokens: usize,
    budget_tokens: usize,
    level: &'static str,
) -> BudgetSkeleton {
    let output_tokens = crate::compress::estimate_tokens(&text);
    BudgetSkeleton {
        text,
        input_tokens,
        output_tokens,
        budget_tokens,
        level,
        budget_met: output_tokens <= budget_tokens,
        // This transform has no durable source publisher. Emitting a marker
        // here would advertise an unresolvable recovery handle.
        recovery_marker: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeletons_rust_fn() {
        let src = "fn a(x:i32)->i32 { x+1 }\nstruct S{n:u8}";
        let out = skeletonize(Path::new("x.rs"), src);
        assert!(out.contains("fn a(x:i32)->i32"), "out: {out}");
        assert!(out.contains("struct S"), "out: {out}");
        assert!(!out.contains("x+1"), "out: {out}");
        assert!(!out.contains('…'), "out: {out}");
        assert!(out.contains("/* body elided */"), "out: {out}");
    }

    #[test]
    fn unsupported_ext_passthrough() {
        let src = "const x = 1;";
        assert_eq!(skeletonize(Path::new("x.zig"), src), src);
    }

    #[test]
    fn budget_skeleton_does_not_claim_unpublished_recovery() {
        let source = "fn example() { println!(\"secret body\"); }";
        let result = skeletonize_to_budget(Path::new("example.rs"), source, 2);
        assert!(result.recovery_marker.is_none());
    }

    /// Per the plan (`docs/plans/2026-07-01-context-engine-unification.md` task 2):
    /// "Diff against `tools/lib/skel.py` on 3 fixtures (a `.rs`, a `.ts`, a `.py`)."
    /// We don't run skel.py from here (it needs `tree_sitter_language_pack`),
    /// but we assert functional parity: signatures preserved, bodies stubbed.
    #[test]
    fn skeletonizes_python_function_and_class() {
        let src = "\
def greet(name):\n    return f\"hello, {name}\"\n\n\
class Foo:\n    def bar(self, x):\n        return x * 2\n";
        let out = skeletonize(Path::new("x.py"), src);
        assert!(out.contains("def greet(name)"), "out: {out}");
        assert!(out.contains("class Foo"), "out: {out}");
        // Function bodies must NOT survive.
        assert!(!out.contains("f\"hello"), "body leaked: {out}");
        assert!(!out.contains("return x * 2"), "body leaked: {out}");
        assert!(!out.contains('…'), "out: {out}");
        assert!(out.contains("pass  # body elided"), "out: {out}");
    }

    #[test]
    fn skeletonizes_typescript_function_and_class() {
        let src = "\
function add(a: number, b: number): number { return a + b; }\n\
class Calculator {\n    multiply(x: number): number { return x * 2; }\n}\n";
        let out = skeletonize(Path::new("x.ts"), src);
        assert!(
            out.contains("function add"),
            "missing function signature in: {out}"
        );
        assert!(out.contains("class Calculator"), "out: {out}");
        // Body content must NOT survive (return / arithmetic).
        assert!(!out.contains("return a + b"), "body leaked: {out}");
        assert!(!out.contains("return x * 2"), "body leaked: {out}");
        assert!(!out.contains('…'), "out: {out}");
        assert!(out.contains("/* body elided */"), "out: {out}");
    }

    #[test]
    fn skeletonizes_javascript_function() {
        let src = "\
function greet(name) {\n    console.log(\"hello, \" + name);\n}\n\
const arrow = (x) => x * 2;\n";
        let out = skeletonize(Path::new("x.js"), src);
        assert!(out.contains("function greet"), "out: {out}");
        // Lexical declarations (const) at top level should also be picked up.
        assert!(out.contains("const arrow"), "out: {out}");
        // `function greet`'s body must be stubbed — `console.log` should NOT
        // survive. (Arrow functions don't have a stubbable body in this
        // implementation; the whole `const arrow = (x) => x * 2;` line is
        // effectively the signature.)
        assert!(!out.contains("console.log"), "body leaked: {out}");
    }

    #[test]
    fn python_empty_input_no_panic() {
        // tree-sitter should handle empty input gracefully.
        let out = skeletonize(Path::new("x.py"), "");
        // Empty input → empty output (or passthrough of "" which is also empty).
        // The key invariant is no panic and a non-broken String return.
        assert!(out.is_empty());
    }

    #[test]
    fn budget_skeleton_degrades_to_path_stub() {
        let src = "fn private_implementation_with_many_arguments(alpha: String, beta: String, gamma: String) { todo!() }";
        let out = skeletonize_to_budget(Path::new("src/internal.rs"), src, 1);
        assert_eq!(out.level, "path-stub");
        assert!(out.text.contains("src/internal.rs"));
        assert!(!out.budget_met, "stub cannot fit one token");
    }
}
