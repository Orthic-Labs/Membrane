use membrane_protocol::{digest_str, CompressionReceiptV1, DroppedSpanV1, NonEmptyString, SpanV1};
use serde::Serialize;
use std::path::{Path, PathBuf};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactorKind {
    Git,
    Tests,
    Build,
    PackageManager,
    Containers,
    Logs,
}
impl CompactorKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "git" => Some(Self::Git),
            "tests" => Some(Self::Tests),
            "build" => Some(Self::Build),
            "package" | "package-manager" => Some(Self::PackageManager),
            "containers" => Some(Self::Containers),
            "logs" => Some(Self::Logs),
            _ => None,
        }
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Tests => "tests",
            Self::Build => "build",
            Self::PackageManager => "package-manager",
            Self::Containers => "containers",
            Self::Logs => "logs",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactOutput {
    pub schema_version: u32,
    pub kind: &'static str,
    pub command_succeeded: bool,
    pub signal_line_count: usize,
    pub omitted_signal_line_count: usize,
    pub summary: String,
    pub raw_digest: String,
    pub raw_pointer: PathBuf,
    pub receipt: CompressionReceiptV1,
}
pub fn compact_and_store(
    kind: CompactorKind,
    raw: &str,
    command_succeeded: bool,
    raw_root: &Path,
    max_lines: usize,
) -> Result<CompactOutput, String> {
    if raw.len() > u32::MAX as usize {
        return Err("raw output exceeds CompressionReceiptV1 span capacity".into());
    }
    let raw_digest = digest_str(raw);
    let raw_path = store_raw(raw_root, &raw_digest, raw)?;
    let lines = line_spans(raw);
    let protect_all = lines
        .iter()
        .any(|(_, _, line)| is_complete_read_marker(line));
    let protected = lines
        .iter()
        .map(|(_, _, line)| protect_all || is_protected(line))
        .collect::<Vec<_>>();
    let mut keep = protected.clone();
    let signal = lines
        .iter()
        .map(|(_, _, line)| is_signal(kind, line))
        .collect::<Vec<_>>();
    let signal_line_count = signal.iter().filter(|value| **value).count();
    let mut selected = 0;
    for (index, value) in signal.iter().enumerate() {
        if *value && !keep[index] && selected < max_lines {
            keep[index] = true;
            selected += 1;
        }
    }
    let head_target = selected + (max_lines - selected).div_ceil(2);
    for index in 0..lines.len() {
        if selected >= head_target {
            break;
        }
        if !keep[index] {
            keep[index] = true;
            selected += 1;
        }
    }
    for index in (0..lines.len()).rev() {
        if selected >= max_lines {
            break;
        }
        if !keep[index] {
            keep[index] = true;
            selected += 1;
        }
    }
    let kept_spans = spans_for(&lines, &keep, true);
    let protected_spans = spans_for(&lines, &protected, true);
    let dropped = spans_for(&lines, &keep, false)
        .into_iter()
        .map(|span| DroppedSpanV1 {
            span,
            reason: nonempty("non_signal_output"),
        })
        .collect::<Vec<_>>();
    let summary = lines
        .iter()
        .zip(&keep)
        .filter_map(|((_, _, line), keep)| keep.then(|| redact_line(line)))
        .collect::<String>();
    let token_delta = -((raw.len().saturating_sub(summary.len()) / 4) as i64);
    let receipt = CompressionReceiptV1 {
        schema_version: CompressionReceiptV1::SCHEMA_VERSION,
        original_hash: raw_digest.clone(),
        transform: nonempty(format!("safe-{}-compactor", kind.as_str())),
        transform_version: nonempty("1"),
        protected_spans,
        kept_spans,
        dropped,
        resolver: nonempty(format!("membrane-compact:get:{raw_digest}")),
        risk: nonempty("low"),
        token_delta,
    };
    Ok(CompactOutput {
        schema_version: 1,
        kind: kind.as_str(),
        command_succeeded,
        signal_line_count,
        omitted_signal_line_count: signal_line_count.saturating_sub(
            keep.iter()
                .zip(&signal)
                .filter(|(kept, signal)| **kept && **signal)
                .count(),
        ),
        summary,
        raw_digest,
        raw_pointer: raw_path
            .strip_prefix(raw_root)
            .expect("stored output remains under raw root")
            .to_path_buf(),
        receipt,
    })
}
pub fn retrieve_raw(raw_root: &Path, digest: &str) -> Result<String, String> {
    validate_digest(digest)?;
    let path = digest_path(raw_root, digest);
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("read raw output {}: {error}", path.display()))?;
    if digest_str(&raw) != digest {
        return Err(format!("raw output hash mismatch for {digest}"));
    }
    Ok(raw)
}
fn store_raw(root: &Path, digest: &str, raw: &str) -> Result<PathBuf, String> {
    let path = digest_path(root, digest);
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|error| format!("create raw output root: {error}"))?;
    if path.exists() {
        retrieve_raw(root, digest)?;
        return Ok(path);
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|error| format!("write raw output: {error}"))?;
    std::fs::rename(&tmp, &path).map_err(|error| format!("activate raw output: {error}"))?;
    Ok(path)
}
fn digest_path(root: &Path, digest: &str) -> PathBuf {
    root.join("sha256")
        .join(format!("{}.raw", digest.trim_start_matches("sha256:")))
}
fn nonempty(value: impl Into<String>) -> NonEmptyString {
    NonEmptyString::new(value).expect("static compactor fields are nonempty")
}
fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() == 71
        && digest.starts_with("sha256:")
        && digest[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err("digest must be sha256:<64 lowercase hex>".into())
    }
}
fn line_spans(raw: &str) -> Vec<(usize, usize, &str)> {
    let mut offset = 0;
    raw.split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, offset, line)
        })
        .collect()
}
fn spans_for(lines: &[(usize, usize, &str)], keep: &[bool], selected: bool) -> Vec<SpanV1> {
    lines
        .iter()
        .zip(keep)
        .filter_map(|((start, end, _), value)| {
            (*value == selected).then_some(SpanV1 {
                start: *start as u32,
                end: *end as u32,
            })
        })
        .collect()
}
fn is_signal(kind: CompactorKind, line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let markers = match kind {
        CompactorKind::Git => "diff --git|@@|modified:|deleted:",
        CompactorKind::Tests => "fail|assert|tests ",
        CompactorKind::Build => "warning|compiling|finished",
        CompactorKind::PackageManager => "err!|warn|vulnerab",
        CompactorKind::Containers => "unhealthy|exited|restart",
        CompactorKind::Logs => "warn|critical|emerg",
    };
    "error|failed|failure|fatal|panic|exception|not ok|conflict"
        .split('|')
        .chain(markers.split('|'))
        .any(|needle| lower.contains(needle))
}
fn is_complete_read_marker(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("[complete-read]")
        || lower.contains("complete_read=true")
        || lower.contains("completeread=true")
        || lower.contains("begin complete read")
}
fn is_protected(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let criteria = [
        "acceptance criteria",
        "criterion",
        "must",
        "shall",
        "required",
        "expected",
    ]
    .iter()
    .any(|word| contains_word(&lower, word));
    let error = [
        "error",
        "failed",
        "failure",
        "fatal",
        "panic",
        "exception",
        "not ok",
    ]
    .iter()
    .any(|word| contains_word(&lower, word));
    let quoted = matches!(
        line.trim_start().chars().next(),
        Some('>' | '"' | '\'' | '`')
    );
    let security = has_advisory(&lower)
        || has_severity(&lower)
        || ["security finding", "vulnerability", "vulnerable"]
            .iter()
            .any(|word| contains_word(&lower, word));
    criteria || error || quoted || security || has_identifier(line) || has_hash(line)
}
#[rustfmt::skip] fn is_word(byte: u8) -> bool { byte.is_ascii_alphanumeric() || byte == b'_' }
#[rustfmt::skip] fn contains_word(text: &str, word: &str) -> bool { text.match_indices(word).any(|(start, _)| { let end = start + word.len(); (start == 0 || !is_word(text.as_bytes()[start - 1])) && (end == text.len() || !is_word(text.as_bytes()[end])) }) }
#[rustfmt::skip] fn has_advisory(text: &str) -> bool { [("cve-", true), ("cwe-", false)].iter().any(|(prefix, cve)| text.match_indices(prefix).any(|(start, _)| { let b = text.as_bytes(); if start > 0 && is_word(b[start - 1]) { return false; } let mut i = start + prefix.len(); if *cve { if b.get(i..i + 5).is_none_or(|v| !v[..4].iter().all(u8::is_ascii_digit) || v[4] != b'-') { return false; } i += 5; } let begin = i; while b.get(i).is_some_and(u8::is_ascii_digit) { i += 1; } i > begin && b.get(i).is_none_or(|v| !is_word(*v)) })) }
#[rustfmt::skip] fn has_severity(text: &str) -> bool { text.match_indices("severity").any(|(start, _)| { let bytes = text.as_bytes(); if start > 0 && is_word(bytes[start - 1]) { return false; } let mut i = start + 8; while bytes.get(i).is_some_and(u8::is_ascii_whitespace) { i += 1; } if !matches!(bytes.get(i), Some(b':' | b'=')) { return false; } i += 1; while bytes.get(i).is_some_and(u8::is_ascii_whitespace) { i += 1; } ["critical", "high", "medium", "low"].iter().any(|level| contains_word(&text[i..], level) && text[i..].starts_with(level)) }) }
fn has_identifier(line: &str) -> bool {
    has_labeled_identifier(line) || has_uuid(line)
}
#[rustfmt::skip] fn has_uuid(line: &str) -> bool { let b = line.as_bytes(); if b.len() < 36 { return false; } (0..=b.len() - 36).any(|start| { let v = &b[start..start + 36]; (start == 0 || !is_word(b[start - 1])) && (start + 36 == b.len() || !is_word(b[start + 36])) && [8, 13, 18, 23].iter().all(|i| v[*i] == b'-') && matches!(v[14].to_ascii_lowercase(), b'1'..=b'5') && matches!(v[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b') && v.iter().enumerate().all(|(i, byte)| [8, 13, 18, 23].contains(&i) || byte.is_ascii_hexdigit()) }) }
fn has_labeled_identifier(line: &str) -> bool {
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
        bytes.windows(key.len()).enumerate().any(|(start, window)| {
            if window != key.as_bytes()
                || (start > 0
                    && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_'))
            {
                return false;
            }
            let mut cursor = start + key.len();
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            if !matches!(bytes.get(cursor), Some(b':' | b'=')) {
                return false;
            }
            cursor += 1;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            bytes
                .get(cursor)
                .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b',' | b';'))
        })
    })
}
fn has_hash(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphanumeric() && c != ':')
        .any(|value| {
            (value.len() == 71
                && value.starts_with("sha256:")
                && value[7..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')))
                || (value.len() == 40
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')))
        })
}
#[rustfmt::skip]
fn redact_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase(); let keyword = "api_key|apikey|token|secret|password|authorization|credential|bearer ".split('|').any(|needle| lower.contains(needle));
    let shaped = line.split(|c: char| c.is_whitespace() || "=:,;\"'()[]{}".contains(c)).any(credential_shape);
    if keyword || shaped { format!("[REDACTED sensitive output]{}", if line.ends_with('\n') { "\n" } else { "" }) } else { line.into() }
}
#[rustfmt::skip]
fn credential_shape(value: &str) -> bool {
    let github = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"].iter().any(|p| value.starts_with(p) && value.len() >= 20); let aws = value.len() == 20 && (value.starts_with("AKIA") || value.starts_with("ASIA")) && value.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()); let jwt = value.starts_with("eyJ") && value.split('.').count() == 3 && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')); github || aws || jwt
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[rustfmt::skip]
    fn protected_evidence_fixtures_bypass_lossy_budget_byte_exactly() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/compression/fixtures/protected-evidence-adversarial.json")).unwrap();
        let root = tempfile::tempdir().unwrap();
        for case in cases.as_array().unwrap() {
            let raw = case["text"].as_str().unwrap();
            let expected = case["expected"].as_str().unwrap();
            let output = compact_and_store(CompactorKind::Logs, raw, true, root.path(), 0).unwrap();
            let bytes = raw.as_bytes();
            let selected = output.receipt.protected_spans.iter()
                .flat_map(|span| bytes[span.start as usize..span.end as usize].iter().copied())
                .collect::<Vec<_>>();
            assert_eq!(selected, expected.as_bytes(), "{}", case["name"]);
            assert!(output.receipt.protected_spans.iter()
                .all(|span| output.receipt.kept_spans.contains(span)));
        }
    }
}
