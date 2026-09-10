//! Native Rust port of `blueprint/src/lib/token-budget.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//! Phase 7.4 — Two-budget tokens for blueprint: `slice_tokens` is a cheap
//! metadata estimate (line-count proxy) usable without reading file bytes;
//! `resolve_tokens` is an honest byte/actual-span estimate computed after a
//! real read. `trim_to_symbol` / `apply_trim_to_symbol` / `chunk_by_syntax`
//! implement trim-to-symbol with a mandatory truncation receipt whenever
//! bytes are dropped, so a silent trim can never occur. All functions here
//! are pure; this module never reads files itself.

const CHARS_PER_TOKEN: f64 = 3.5;
pub const MAX_RETAINED_BYTES: usize = 256 * 1024;

/// A token is ~3.5 chars for prose / source code. Mirrors `charsToTokens`.
pub fn chars_to_tokens(chars: f64) -> i64 {
    if !chars.is_finite() || chars <= 0.0 {
        return 1;
    }
    ((chars / CHARS_PER_TOKEN).round() as i64).max(1)
}

/// Metadata for [`slice_tokens`]: only the line span matters. Mirrors the
/// JS `evidence` object's `{ startLine, endLine }` fields (others ignored).
#[derive(Debug, Clone, Copy, Default)]
pub struct SliceEvidence {
    pub start_line: Option<f64>,
    pub end_line: Option<f64>,
}

/// Slice-time metadata estimate: cheap, deterministic, no file reads.
/// Mirrors `sliceTokens`. Returns an integer >= 1.
pub fn slice_tokens(evidence: Option<&SliceEvidence>) -> i64 {
    let evidence = match evidence {
        Some(e) => e,
        None => return 1,
    };
    let start_line = evidence.start_line.unwrap_or(1.0);
    let end_line = evidence.end_line.unwrap_or(start_line);
    ((end_line - start_line + 1.0) as i64).max(1)
}

/// Result of [`resolve_tokens`]. Mirrors the JS return shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTokens {
    pub tokens: i64,
    pub bytes: usize,
    pub lines: usize,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_line: i64,
    pub end_line: i64,
}

/// Options for [`resolve_tokens`], mirroring the JS `options` object.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveOptions {
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
}

/// Resolution-time byte/actual-span estimate. Mirrors `resolveTokens`.
/// Returns an integer >= 1 token count, plus byte/line counts.
pub fn resolve_tokens(actual_text: &str, options: ResolveOptions) -> ResolvedTokens {
    if actual_text.is_empty() {
        return ResolvedTokens {
            tokens: 1,
            bytes: 0,
            lines: 0,
            span: None,
        };
    }
    let bytes = actual_text.len(); // UTF-8 byte length, matches Buffer.byteLength(text, "utf8")
    let lines = split_lines(actual_text).len();
    let tokens = chars_to_tokens(if bytes > 0 { bytes as f64 } else { actual_text.chars().count() as f64 });
    ResolvedTokens {
        tokens,
        bytes,
        lines,
        span: Some(Span {
            start_line: options.start_line.unwrap_or(1),
            end_line: options.end_line.unwrap_or(lines as i64),
        }),
    }
}

/// Split on `\r?\n`, matching the JS `text.split(/\r?\n/)`.
fn split_lines(text: &str) -> Vec<&str> {
    // Normalize \r\n to \n conceptually by splitting on \n and trimming a
    // trailing \r from each piece, matching /\r?\n/ semantics.
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Result of [`trim_to_symbol`]. Mirrors the JS return shape.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimResult {
    pub start_line: i64,
    pub end_line: i64,
    pub trimmed: bool,
    pub reason: &'static str,
}

/// Snap a `[start_line..end_line]` span (1-indexed inclusive) to the
/// nearest enclosing `{` / `}`-or-`;` boundaries. Mirrors `trimToSymbol`.
pub fn trim_to_symbol(lines: &[&str], start_line: i64, end_line: i64) -> TrimResult {
    if lines.is_empty() {
        return TrimResult {
            start_line: 1,
            end_line: 1,
            trimmed: false,
            reason: "no_lines",
        };
    }
    let total = lines.len() as i64;
    let in_start = start_line.max(1).min(total);
    let in_end = end_line.max(in_start).min(total);

    let mut brace_start: i64 = -1;
    let mut i = in_start - 1;
    while i >= 0 {
        if lines[i as usize].contains('{') {
            brace_start = i;
            break;
        }
        i -= 1;
    }

    let mut brace_end: i64 = -1;
    let mut i = in_end - 1;
    while i < total {
        let line = lines[i as usize];
        if line.contains('}') || line.trim_end().ends_with(';') {
            brace_end = i;
            break;
        }
        i += 1;
    }

    let out_start = if brace_start >= 0 { brace_start + 1 } else { in_start };
    let out_end = if brace_end >= 0 { brace_end + 1 } else { in_end };
    let trimmed = out_start != in_start || out_end != in_end;
    let reason = if brace_start >= 0 && brace_end >= 0 {
        "snapped_to_braces"
    } else if brace_start >= 0 {
        "snapped_to_open_brace"
    } else if brace_end >= 0 {
        "snapped_to_close_or_semicolon"
    } else {
        "no_trim_needed"
    };

    TrimResult {
        start_line: out_start,
        end_line: out_end,
        trimmed,
        reason,
    }
}

/// A truncation receipt, mirroring `truncationReceipt`'s output shape.
#[derive(Debug, Clone, PartialEq)]
pub struct TruncationReceipt {
    pub schema: &'static str, // "blueprint.truncation.v1"
    pub retained_bytes: usize,
    pub original_bytes: usize,
    pub retained_lines: usize,
    pub original_lines: usize,
    pub retained_tokens: i64,
    pub original_tokens: i64,
    pub reason: String,
    pub symbol_boundary: bool,
    pub trimmed_at: String,
}

#[derive(Debug)]
pub struct TokenBudgetError(pub String);
impl std::fmt::Display for TokenBudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for TokenBudgetError {}

/// Options for [`truncation_receipt`].
#[derive(Debug, Clone, Default)]
pub struct TruncationReceiptOptions {
    pub reason: Option<String>,
    pub symbol_boundary: bool,
}

fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let (h, m, s) = (time_of_day / 3600, (time_of_day % 3600) / 60, time_of_day % 60);
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mth <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", year, mth, d, h, m, s, millis)
}

/// Build a truncation receipt. Mirrors `truncationReceipt`. Errors (rather
/// than panics) when `retained.bytes >= original.bytes`, matching the JS
/// "caller bug" throw.
pub fn truncation_receipt(
    retained: &ResolvedTokens,
    original: &ResolvedTokens,
    options: TruncationReceiptOptions,
) -> Result<TruncationReceipt, TokenBudgetError> {
    if retained.bytes >= original.bytes {
        return Err(TokenBudgetError(
            "truncationReceipt called when no truncation occurred — caller bug".to_string(),
        ));
    }
    Ok(TruncationReceipt {
        schema: "blueprint.truncation.v1",
        retained_bytes: retained.bytes,
        original_bytes: original.bytes,
        retained_lines: retained.lines,
        original_lines: original.lines,
        retained_tokens: retained.tokens,
        original_tokens: original.tokens,
        reason: options.reason.unwrap_or_else(|| "byte_cap".to_string()),
        symbol_boundary: options.symbol_boundary,
        trimmed_at: now_iso8601(),
    })
}

/// Result of [`apply_trim_to_symbol`]. Mirrors the JS return shape (with
/// `original_text`/`original` populated only on the byte-cap-trimmed path,
/// matching the JS object which omits those keys otherwise).
#[derive(Debug, Clone)]
pub struct AppliedTrim {
    pub text: String,
    pub original_text: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub resolve: ResolvedTokens,
    pub original: Option<ResolvedTokens>,
    pub receipt: Option<TruncationReceipt>,
    pub symbol_boundary: bool,
}

/// Options for [`apply_trim_to_symbol`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ApplyTrimOptions {
    pub max_bytes: Option<usize>,
}

fn byte_len_with_newline(line: &str) -> usize {
    line.len() + 1 // "\n" appended, as in the JS `Buffer.byteLength(lines[i] + "\n", "utf8")`
}

/// Trim `lines[startLine..endLine]` (1-indexed inclusive) to a symbol
/// boundary, then to `max_bytes` if needed, returning text + receipt.
/// Mirrors `applyTrimToSymbol`.
pub fn apply_trim_to_symbol(
    lines: &[&str],
    start_line: i64,
    end_line: i64,
    options: ApplyTrimOptions,
) -> Result<AppliedTrim, TokenBudgetError> {
    let max_bytes = options.max_bytes.unwrap_or(MAX_RETAINED_BYTES);
    let snapped = trim_to_symbol(lines, start_line, end_line);

    let text = slice_join(lines, snapped.start_line, snapped.end_line);
    let original = resolve_tokens(
        &text,
        ResolveOptions {
            start_line: Some(snapped.start_line),
            end_line: Some(snapped.end_line),
        },
    );

    if original.bytes <= max_bytes {
        return Ok(AppliedTrim {
            text,
            original_text: None,
            start_line: snapped.start_line,
            end_line: snapped.end_line,
            resolve: original,
            original: None,
            receipt: None,
            symbol_boundary: snapped.trimmed,
        });
    }

    let mut retained_bytes = 0usize;
    let mut retained_end = snapped.end_line;
    for i in (snapped.start_line - 1)..snapped.end_line {
        let line_bytes = byte_len_with_newline(lines[i as usize]);
        if retained_bytes + line_bytes > max_bytes {
            break;
        }
        retained_bytes += line_bytes;
        retained_end = i + 1;
    }

    let trimmed_text = slice_join(lines, snapped.start_line, retained_end);
    let retained = resolve_tokens(
        &trimmed_text,
        ResolveOptions {
            start_line: Some(snapped.start_line),
            end_line: Some(retained_end),
        },
    );
    let receipt = truncation_receipt(
        &retained,
        &original,
        TruncationReceiptOptions {
            reason: Some("byte_cap".to_string()),
            symbol_boundary: snapped.trimmed,
        },
    )?;

    Ok(AppliedTrim {
        text: trimmed_text,
        original_text: Some(text),
        start_line: snapped.start_line,
        end_line: retained_end,
        resolve: retained,
        original: Some(original),
        receipt: Some(receipt),
        symbol_boundary: snapped.trimmed,
    })
}

fn slice_join(lines: &[&str], start_line: i64, end_line: i64) -> String {
    let start_idx = (start_line - 1).max(0) as usize;
    let end_idx = end_line.max(0) as usize;
    if start_idx >= lines.len() {
        return String::new();
    }
    let end_idx = end_idx.min(lines.len());
    lines[start_idx..end_idx].join("\n")
}

/// Symbol span input for [`chunk_by_syntax`], mirroring the JS
/// `{ startLine, endLine, parent: { startLine, endLine } }` shape.
#[derive(Debug, Clone, Copy, Default)]
pub struct SyntaxSymbol {
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub parent_start_line: Option<i64>,
    pub parent_end_line: Option<i64>,
}

/// Result of [`chunk_by_syntax`]. Mirrors [`AppliedTrim`]'s shape.
pub type SyntaxChunk = AppliedTrim;

/// D27: symbol/parent-aware chunk resolution. Trims only at the enclosing
/// symbol's exact span and emits a mandatory truncation receipt when bytes
/// are dropped. Mirrors `chunkBySyntax`.
pub fn chunk_by_syntax(
    text: &str,
    symbol: Option<&SyntaxSymbol>,
    max_bytes: Option<usize>,
) -> Result<SyntaxChunk, TokenBudgetError> {
    let max_bytes = max_bytes.unwrap_or(MAX_RETAINED_BYTES);
    let lines = split_lines(text);

    let symbol_span = symbol.and_then(|s| {
        s.start_line.map(|sl| Span {
            start_line: sl,
            end_line: s.end_line.unwrap_or(sl),
        })
    });
    let parent_span = symbol.and_then(|s| {
        s.parent_start_line.map(|sl| Span {
            start_line: sl,
            end_line: s.parent_end_line.unwrap_or(sl),
        })
    });
    let span = symbol_span
        .or(parent_span)
        .unwrap_or(Span {
            start_line: 1,
            end_line: lines.len() as i64,
        });

    let total = lines.len() as i64;
    let bounded = Span {
        start_line: span.start_line.max(1).min(total.max(1)),
        end_line: span.end_line.max(span.start_line).min(total.max(1)),
    };

    let chunk = slice_join(&lines, bounded.start_line, bounded.end_line);
    let original = resolve_tokens(
        &chunk,
        ResolveOptions {
            start_line: Some(bounded.start_line),
            end_line: Some(bounded.end_line),
        },
    );
    let has_symbol_boundary = symbol_span.is_some();

    if original.bytes <= max_bytes {
        return Ok(AppliedTrim {
            text: chunk,
            original_text: None,
            start_line: bounded.start_line,
            end_line: bounded.end_line,
            resolve: original,
            original: None,
            receipt: None,
            symbol_boundary: has_symbol_boundary,
        });
    }

    let mut retained_bytes = 0usize;
    let mut retained_end = bounded.end_line;
    for i in (bounded.start_line - 1)..bounded.end_line {
        let line_bytes = byte_len_with_newline(lines[i as usize]);
        if retained_bytes + line_bytes > max_bytes {
            break;
        }
        retained_bytes += line_bytes;
        retained_end = i + 1;
    }

    let trimmed_text = slice_join(&lines, bounded.start_line, retained_end);
    let retained = resolve_tokens(
        &trimmed_text,
        ResolveOptions {
            start_line: Some(bounded.start_line),
            end_line: Some(retained_end),
        },
    );
    let receipt = truncation_receipt(
        &retained,
        &original,
        TruncationReceiptOptions {
            reason: Some("syntax_chunk_byte_cap".to_string()),
            symbol_boundary: has_symbol_boundary,
        },
    )?;

    Ok(AppliedTrim {
        text: trimmed_text,
        original_text: Some(chunk),
        start_line: bounded.start_line,
        end_line: retained_end,
        resolve: retained,
        original: Some(original),
        receipt: Some(receipt),
        symbol_boundary: has_symbol_boundary,
    })
}
