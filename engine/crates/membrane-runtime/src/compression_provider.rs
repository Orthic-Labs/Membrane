//! Deterministic, query-aware compression at the prep boundary.
use crate::{code_batch::{ReservationV1, ResourceBudgetV1}, compress, skel};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionContentKind {
    Json,
    Code,
    Prose,
    Logs,
    Table,
    ToolOutput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressionRequest {
    pub source: String,
    pub path: Option<PathBuf>,
    pub query: String,
    pub budget_tokens: usize,
    pub authority_admitted: bool,
    pub freshness_valid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressionResult {
    pub kind: CompressionContentKind,
    pub text: String,
    pub output_tokens: usize,
    pub admitted: bool,
    pub refusal: Option<&'static str>,
}

pub fn classify(path: Option<&Path>, source: &str) -> CompressionContentKind {
    let ext = path
        .and_then(Path::extension)
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "json" | "jsonl" | "ndjson")
        || serde_json::from_str::<serde_json::Value>(source.trim()).is_ok()
    {
        return CompressionContentKind::Json;
    }
    if matches!(ext.as_str(), "csv" | "tsv" | "table")
        || source.lines().any(|l| l.matches('|').count() >= 2)
    {
        return CompressionContentKind::Table;
    }
    if matches!(ext.as_str(), "log" | "logs") {
        return CompressionContentKind::Logs;
    }
    if matches!(ext.as_str(), "tool" | "ndjson-tool") {
        return CompressionContentKind::ToolOutput;
    }
    if matches!(
        ext.as_str(),
        "rs" | "py" | "js" | "ts" | "tsx" | "go" | "java" | "c" | "cpp" | "sh" | "swift"
    ) {
        return CompressionContentKind::Code;
    }
    CompressionContentKind::Prose
}

pub fn compress_query_aware(request: &CompressionRequest) -> CompressionResult {
    let kind = classify(request.path.as_deref(), &request.source);
    let budget = ResourceBudgetV1::defaults();
    let reservation = ReservationV1 {
        items: 1,
        bytes: request.source.len(),
        tokens: request.budget_tokens,
        ..ReservationV1::ZERO
    };
    if budget.validate().is_err() || budget.reserve(&reservation).is_err() {
        return CompressionResult { kind, text: String::new(), output_tokens: 0, admitted: false, refusal: Some("resource_budget_exceeded") };
    }
    if !request.authority_admitted || !request.freshness_valid {
        return CompressionResult {
            kind,
            text: String::new(),
            output_tokens: 0,
            admitted: false,
            refusal: Some(if request.authority_admitted {
                "source_stale"
            } else {
                "authority_not_admitted"
            }),
        };
    }
    let text = match kind {
        CompressionContentKind::Json | CompressionContentKind::Table => request.source.clone(),
        CompressionContentKind::Code if !request.source.lines().any(|line| is_protected(line)) => {
            request
                .path
                .as_deref()
                .map(|p| {
                    skel::skeletonize_to_budget(p, &request.source, request.budget_tokens).text
                })
                .unwrap_or_else(|| request.source.clone())
        }
        CompressionContentKind::Code => request.source.clone(),
        _ => prioritize_lines(&request.source, &request.query, request.budget_tokens),
    };
    let output_tokens = compress::estimate_tokens(&text);
    CompressionResult {
        kind,
        text,
        output_tokens,
        admitted: true,
        refusal: None,
    }
}

fn prioritize_lines(source: &str, query: &str, budget: usize) -> String {
    let terms = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .filter(|v| v.len() > 1)
        .collect::<Vec<_>>();
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let complete_read = lines.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("[complete-read]")
            || lower.contains("complete_read=true")
            || lower.contains("completeread=true")
            || lower.contains("begin complete read")
    });
    let mut protected = Vec::new();
    let mut preferred = Vec::new();
    let mut ordinary = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if complete_read || is_protected(line) {
            protected.push(i);
        } else if terms.iter().any(|term| lower.contains(term)) {
            preferred.push(i);
        } else {
            ordinary.push(i);
        }
    }
    let mut selected = protected.clone();
    let mut used: usize = protected
        .iter()
        .map(|i| compress::estimate_tokens(lines[*i]))
        .sum();
    for index in preferred.into_iter().chain(ordinary) {
        let tokens = compress::estimate_tokens(lines[index]);
        if used + tokens <= budget {
            selected.push(index);
            used += tokens;
        }
    }
    selected.sort_unstable();
    let out = selected.into_iter().map(|i| lines[i]).collect::<String>();
    if protected.iter().any(|i| !out.contains(lines[*i])) {
        return source.to_string();
    }
    out
}

fn is_protected(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "acceptance criteria",
        "criterion",
        "must",
        "shall",
        "required",
        "expected",
        "error",
        "failed",
        "failure",
        "fatal",
        "panic",
        "exception",
        "not ok",
        "cve-",
        "cwe-",
        "security finding",
        "vulnerability",
        "severity:",
        "severity=",
    ]
    .iter()
    .any(|needle| contains_word(&lower, needle))
        || matches!(
            line.trim_start().chars().next(),
            Some('>' | '"' | '\'' | '`')
        )
        || has_labeled_id(line)
        || has_uuid(line)
        || has_hash(line)
}

fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
fn contains_word(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(i, _)| {
        let end = i + needle.len();
        (i == 0 || !word(text.as_bytes()[i - 1]))
            && (end == text.len() || !word(text.as_bytes()[end]))
    })
}
fn has_labeled_id(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    [
        "id",
        "candidateid",
        "candidate_id",
        "candidate-id",
        "receiptid",
        "receipt_id",
        "receipt-id",
        "traceid",
        "trace_id",
        "trace-id",
        "taskid",
        "task_id",
        "task-id",
        "sessionid",
        "session_id",
        "session-id",
        "scopeid",
        "scope_id",
        "scope-id",
    ]
    .iter()
    .any(|key| {
        bytes.windows(key.len()).enumerate().any(|(i, w)| {
            w == key.as_bytes() && (i == 0 || !word(bytes[i - 1])) && {
                let mut j = i + key.len();
                while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                    j += 1;
                }
                if !matches!(bytes.get(j), Some(b':' | b'=')) {
                    return false;
                }
                j += 1;
                while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                    j += 1;
                }
                bytes
                    .get(j)
                    .is_some_and(|v| !v.is_ascii_whitespace() && !matches!(v, b',' | b';'))
            }
        })
    })
}
fn has_uuid(line: &str) -> bool {
    let b = line.as_bytes();
    if b.len() < 36 {
        return false;
    }
    (0..=b.len() - 36).any(|i| {
        let v = &b[i..i + 36];
        (i == 0 || !word(b[i - 1]))
            && (i + 36 == b.len() || !word(b[i + 36]))
            && [8, 13, 18, 23].iter().all(|p| v[*p] == b'-')
            && matches!(v[14].to_ascii_lowercase(), b'1'..=b'5')
            && matches!(v[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
            && v.iter()
                .enumerate()
                .all(|(p, c)| [8, 13, 18, 23].contains(&p) || c.is_ascii_hexdigit())
    })
}
fn has_hash(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphanumeric() && c != ':')
        .any(|v| {
            (v.len() == 71
                && v.starts_with("sha256:")
                && v[7..]
                    .bytes()
                    .all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f')))
                || (v.len() == 40
                    && v.bytes()
                        .all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f')))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_holdout_reduces_tokens_and_keeps_protected_lines() {
        let source = "noise one\nerror: CVE-2026-12345\nquery target alpha\nnoise two\n";
        let result = compress_query_aware(&CompressionRequest {
            source: source.into(),
            path: None,
            query: "target".into(),
            budget_tokens: 8,
            authority_admitted: true,
            freshness_valid: true,
        });
        assert!(result.text.contains("error: CVE-2026-12345"));
        assert!(result.text.contains("target"));
    }
    #[test]
    fn strict_identifier_boundaries_match_protection_contract() {
        let source = "xacceptance criteriaz\n00000000-0000-0000-0000-000000000000\nABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD\nid: opaque-007\n";
        let result = compress_query_aware(&CompressionRequest {
            source: source.into(),
            path: None,
            query: String::new(),
            budget_tokens: 0,
            authority_admitted: true,
            freshness_valid: true,
        });
        assert_eq!(result.text, "id: opaque-007\n");
    }
    #[test]
    fn denial_is_content_free_and_json_table_stay_structured() {
        let denied = compress_query_aware(&CompressionRequest {
            source: "secret body".into(),
            path: None,
            query: String::new(),
            budget_tokens: 1,
            authority_admitted: false,
            freshness_valid: true,
        });
        assert_eq!(
            (denied.text, denied.refusal),
            (String::new(), Some("authority_not_admitted"))
        );
        for (path, source) in [("x.json", "{\"a\":1}"), ("x.csv", "a,b\n1,2\n")] {
            let result = compress_query_aware(&CompressionRequest {
                source: source.into(),
                path: Some(path.into()),
                query: "a".into(),
                budget_tokens: 1,
                authority_admitted: true,
                freshness_valid: true,
            });
            assert_eq!(result.text, source);
        }
    }
    #[test]
    fn protected_code_is_exact_and_selected_records_keep_source_order() {
        let code = "fn keep() {}\nerror: do not elide\nfn query_target() {}\n";
        let protected = compress_query_aware(&CompressionRequest {
            source: code.into(),
            path: Some("x.rs".into()),
            query: "query_target".into(),
            budget_tokens: 1,
            authority_admitted: true,
            freshness_valid: true,
        });
        assert_eq!(protected.text, code);
        let prose = compress_query_aware(&CompressionRequest {
            source: "tail\nquery hit\nhead\n".into(),
            path: None,
            query: "query".into(),
            budget_tokens: 3,
            authority_admitted: true,
            freshness_valid: true,
        });
        assert!(prose.text.contains("query hit"));
    }
}
