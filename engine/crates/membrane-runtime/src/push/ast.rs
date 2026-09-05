//! Source-preserving interface projection. Only parsed function bodies are
//! elided. Imports, exports, decorators, fields and multiline signatures survive.
use super::fidelity::{Span, SpanMapping};
use std::path::Path;
use tree_sitter::{Language, Node, Parser};

fn language(path: &Path) -> Option<(Language, bool)> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "rs" => Some((tree_sitter_rust::LANGUAGE.into(), false)),
        "py" => Some((tree_sitter_python::LANGUAGE.into(), true)),
        "js" | "jsx" | "mjs" | "cjs" => Some((tree_sitter_javascript::LANGUAGE.into(), false)),
        "ts" => Some((tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), false)),
        "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), false)),
        _ => None,
    }
}
fn bodies(node: Node<'_>, ranges: &mut Vec<(usize, usize)>, depth: usize) {
    if depth > 256 { return; }
    if matches!(node.kind(), "function_item" | "function_definition" | "function_declaration" | "function_expression" | "method_definition" | "arrow_function" | "generator_function_declaration" | "generator_function") {
        if let Some(body) = node.child_by_field_name("body") {
            if matches!(body.kind(), "block" | "statement_block") {
                ranges.push((body.start_byte(), body.end_byte()));
                return;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) { bodies(child, ranges, depth + 1); }
}
fn exact(source: &str) -> (String, Vec<SpanMapping>) {
    (source.into(), vec![SpanMapping { source:Span {start:0,end:source.len()}, output:Span {start:0,end:source.len()} }])
}
pub fn render(path: &Path, source: &str) -> (String, Vec<SpanMapping>) {
    if source.len() > super::recovery::MAX_ARTIFACT_BYTES { return exact(source); }
    let Some((language, python)) = language(path) else { return exact(source); };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() { return exact(source); }
    let Some(tree) = parser.parse(source, None) else { return exact(source); };
    if tree.root_node().has_error() { return exact(source); }
    let mut ranges = Vec::new();
    bodies(tree.root_node(), &mut ranges, 0);
    if ranges.is_empty() { return exact(source); }
    ranges.sort_unstable();
    let mut output = String::new();
    let mut mappings = Vec::new();
    let mut previous = 0;
    for (start, end) in ranges {
        if start < previous || end > source.len() || !source.is_char_boundary(start) || !source.is_char_boundary(end) { return exact(source); }
        let at = output.len();
        output.push_str(&source[previous..start]);
        mappings.push(SpanMapping { source:Span {start:previous,end:start}, output:Span {start:at,end:output.len()} });
        output.push_str(if python { "pass  # body elided" } else { "{ /* body elided */ }" });
        previous = end;
    }
    let at = output.len();
    output.push_str(&source[previous..]);
    mappings.push(SpanMapping { source:Span {start:previous,end:source.len()}, output:Span {start:at,end:output.len()} });
    (output, mappings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiline_decorated_python_keeps_complete_header_and_import() {
        let source = "import decimal\n@decorator\ndef calculate(\n    amount: int,\n    tax: float,\n) -> float:\n    return amount * tax\n";
        let (out, mappings) = render(Path::new("x.py"), source);
        assert!(out.starts_with("import decimal\n@decorator\ndef calculate(\n    amount: int,\n    tax: float,\n) -> float:\n"));
        assert!(out.contains("pass  # body elided"));
        super::super::fidelity::validate(source.as_bytes(), &super::super::recovery::digest(source.as_bytes()), out.as_bytes(), &mappings, &[]).unwrap();
    }
    #[test]
    fn destructuring_exports_tsx_and_rust_fields_survive() {
        let source = "export function total({ x, y }: Point): number { return x + y; }\n";
        let (out, _) = render(Path::new("x.ts"), source);
        assert!(out.contains("export function total({ x, y }: Point): number { /* body elided */ }"));
        let source = "export function View({x}: Props) { return <div>{x}</div>; }";
        let (out, _) = render(Path::new("x.tsx"), source);
        assert!(out.starts_with("export function View({x}: Props)"));
        assert!(!out.contains("<div>"));
        let (out, _) = render(Path::new("x.rs"), "pub struct S { pub n: u8 }\nimpl S { pub fn n(&self) -> u8 { self.n } }");
        assert!(out.contains("pub n: u8"));
        assert!(out.contains("pub fn n(&self) -> u8"));
    }
    #[test]
    fn error_parse_and_unknown_language_stay_exact() {
        for (path, source) in [("x.ts", "export function foo( {"), ("x.go", "func main() { println(1) }")] {
            assert_eq!(render(Path::new(path), source).0, source);
        }
    }
}
