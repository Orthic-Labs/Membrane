//! Parity tests for `lib_token_budget` (native port of
//! `blueprint/src/lib/token-budget.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_token_budget::{
    apply_trim_to_symbol, chars_to_tokens, chunk_by_syntax, resolve_tokens, slice_tokens,
    trim_to_symbol, truncation_receipt, ApplyTrimOptions, ResolveOptions, SliceEvidence,
    SyntaxSymbol, TruncationReceiptOptions,
};

#[test]
fn chars_to_tokens_rounds_and_floors_at_one() {
    assert_eq!(chars_to_tokens(0.0), 1);
    assert_eq!(chars_to_tokens(-5.0), 1);
    assert_eq!(chars_to_tokens(f64::NAN), 1);
    assert_eq!(chars_to_tokens(3.5), 1);
    assert_eq!(chars_to_tokens(7.0), 2);
    assert_eq!(chars_to_tokens(35.0), 10);
}

#[test]
fn slice_tokens_defaults_and_line_span() {
    assert_eq!(slice_tokens(None), 1);
    assert_eq!(
        slice_tokens(Some(&SliceEvidence { start_line: Some(10.0), end_line: Some(15.0) })),
        6
    );
    assert_eq!(
        slice_tokens(Some(&SliceEvidence { start_line: None, end_line: None })),
        1
    );
}

#[test]
fn resolve_tokens_empty_text_is_one_token_zero_bytes() {
    let r = resolve_tokens("", ResolveOptions::default());
    assert_eq!(r.tokens, 1);
    assert_eq!(r.bytes, 0);
    assert_eq!(r.lines, 0);
    assert_eq!(r.span, None);
}

#[test]
fn resolve_tokens_counts_bytes_and_lines() {
    let text = "line one\nline two\nline three";
    let r = resolve_tokens(text, ResolveOptions { start_line: Some(5), end_line: Some(7) });
    assert_eq!(r.bytes, text.len());
    assert_eq!(r.lines, 3);
    assert!(r.tokens >= 1);
    assert_eq!(r.span.unwrap().start_line, 5);
    assert_eq!(r.span.unwrap().end_line, 7);
}

#[test]
fn trim_to_symbol_snaps_to_braces() {
    let lines = vec![
        "fn foo() {",      // 1
        "    let x = 1;",  // 2
        "    bar();",      // 3
        "}",               // 4
        "fn baz() {}",     // 5
    ];
    let result = trim_to_symbol(&lines, 2, 3);
    // Backward from line 2 finds the opening `{` on line 1. Forward from
    // line 3 ("    bar();") itself already ends with `;`, so the forward
    // scan stops immediately on line 3 -- it does not walk to the `}` on
    // line 4.
    assert_eq!(result.start_line, 1);
    assert_eq!(result.end_line, 3);
    assert!(result.trimmed);
    assert_eq!(result.reason, "snapped_to_braces");
}

#[test]
fn trim_to_symbol_no_trim_when_already_on_boundary() {
    let lines = vec!["fn foo() {", "}"];
    let result = trim_to_symbol(&lines, 1, 2);
    assert_eq!(result.start_line, 1);
    assert_eq!(result.end_line, 2);
    assert!(!result.trimmed);
}

#[test]
fn trim_to_symbol_empty_lines_yields_no_lines_reason() {
    let lines: Vec<&str> = vec![];
    let result = trim_to_symbol(&lines, 1, 1);
    assert_eq!(result.reason, "no_lines");
    assert!(!result.trimmed);
}

#[test]
fn truncation_receipt_requires_actual_shrinkage() {
    let bigger = resolve_tokens("aaaaaaaaaa", ResolveOptions::default());
    let smaller = resolve_tokens("aaa", ResolveOptions::default());
    let receipt = truncation_receipt(&smaller, &bigger, TruncationReceiptOptions::default())
        .expect("valid truncation");
    assert_eq!(receipt.schema, "blueprint.truncation.v1");
    assert_eq!(receipt.retained_bytes, 3);
    assert_eq!(receipt.original_bytes, 10);
    assert_eq!(receipt.reason, "byte_cap");

    let err = truncation_receipt(&bigger, &smaller, TruncationReceiptOptions::default());
    assert!(err.is_err(), "must error when retained >= original (caller bug)");
}

#[test]
fn apply_trim_to_symbol_below_cap_has_no_receipt() {
    let lines = vec!["fn foo() {", "  1;", "}"];
    let result = apply_trim_to_symbol(&lines, 2, 2, ApplyTrimOptions::default()).unwrap();
    // Forward scan from line 2 ("  1;") itself ends with `;`, so it stops
    // there rather than reaching the `}` on line 3.
    assert!(result.receipt.is_none());
    assert_eq!(result.start_line, 1);
    assert_eq!(result.end_line, 2);
    assert!(result.symbol_boundary);
}

#[test]
fn apply_trim_to_symbol_over_cap_produces_receipt_and_shrinks() {
    let long_line = "x".repeat(100);
    let lines_owned: Vec<String> = (0..10).map(|_| long_line.clone()).collect();
    let mut all = vec!["fn foo() {".to_string()];
    all.extend(lines_owned);
    all.push("}".to_string());
    let lines: Vec<&str> = all.iter().map(|s| s.as_str()).collect();

    let result = apply_trim_to_symbol(
        &lines,
        2,
        3,
        ApplyTrimOptions { max_bytes: Some(150) },
    )
    .unwrap();
    assert!(result.receipt.is_some());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.reason, "byte_cap");
    assert!(receipt.retained_bytes < receipt.original_bytes);
    assert!(result.original_text.is_some());
}

#[test]
fn chunk_by_syntax_uses_symbol_span_when_present() {
    let text = "line1\nline2\nline3\nline4\nline5";
    let symbol = SyntaxSymbol {
        start_line: Some(2),
        end_line: Some(4),
        ..Default::default()
    };
    let chunk = chunk_by_syntax(text, Some(&symbol), None).unwrap();
    assert_eq!(chunk.start_line, 2);
    assert_eq!(chunk.end_line, 4);
    assert_eq!(chunk.text, "line2\nline3\nline4");
    assert!(chunk.symbol_boundary);
    assert!(chunk.receipt.is_none());
}

#[test]
fn chunk_by_syntax_falls_back_to_parent_span_then_whole_text() {
    let text = "a\nb\nc";
    let with_parent = SyntaxSymbol {
        parent_start_line: Some(1),
        parent_end_line: Some(2),
        ..Default::default()
    };
    let chunk = chunk_by_syntax(text, Some(&with_parent), None).unwrap();
    assert_eq!(chunk.text, "a\nb");
    assert!(!chunk.symbol_boundary);

    let no_symbol = chunk_by_syntax(text, None, None).unwrap();
    assert_eq!(no_symbol.text, "a\nb\nc");
    assert!(!no_symbol.symbol_boundary);
}

#[test]
fn chunk_by_syntax_over_cap_produces_receipt_with_syntax_reason() {
    let long_line = "y".repeat(100);
    let text = format!("{}\n{}\n{}", long_line, long_line, long_line);
    let symbol = SyntaxSymbol {
        start_line: Some(1),
        end_line: Some(3),
        ..Default::default()
    };
    let chunk = chunk_by_syntax(&text, Some(&symbol), Some(150)).unwrap();
    let receipt = chunk.receipt.expect("expected truncation receipt");
    assert_eq!(receipt.reason, "syntax_chunk_byte_cap");
    assert!(receipt.symbol_boundary);
}
