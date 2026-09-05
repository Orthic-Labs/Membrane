//! L3 code skeletonizer — extract signatures/decls from source files.
//!
//! v1 supports a small set of common languages (Rust/Python/JS/TS). Unsupported
//! extensions pass through unchanged (parity with the workspace `skel.py`
//! fallback).

use std::path::Path;


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
    pub recovery_marker: Option<crate::push::compress::RecoveryMarkerV1>,
}

pub fn skeletonize_with_spans(path: &Path, src: &str) -> (String, Vec<super::fidelity::SpanMapping>) {
    super::ast::render(path, src)
}
pub fn skeletonize(path: &Path, src: &str) -> String { skeletonize_with_spans(path, src).0 }
pub fn skeletonize_to_budget(path: &Path, src: &str, budget_tokens: usize) -> BudgetSkeleton {
    let input_tokens = super::compress::estimate_tokens(src);
    let candidate = skeletonize(path, src);
    let candidate_tokens = super::compress::estimate_tokens(&candidate);
    let (text, level) = if candidate_tokens <= budget_tokens {
        (candidate, "signature")
    } else {
        // A path alone is not a retained original. Keep exact source and make
        // the unsatisfied budget observable to the final delivery owner.
        (src.to_owned(), "exact-over-budget")
    };
    let output_tokens = super::compress::estimate_tokens(&text);
    BudgetSkeleton { text, input_tokens, output_tokens, budget_tokens, level,
        budget_met:output_tokens <= budget_tokens, recovery_marker:None }
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
    fn budget_skeleton_keeps_exact_when_no_faithful_view_fits() {
        let src = "fn private_implementation_with_many_arguments(alpha: String, beta: String, gamma: String) { todo!() }";
        let out = skeletonize_to_budget(Path::new("src/internal.rs"), src, 1);
        assert_eq!(out.level, "exact-over-budget");
        assert_eq!(out.text, src);
        assert!(!out.budget_met, "stub cannot fit one token");
    }
}
