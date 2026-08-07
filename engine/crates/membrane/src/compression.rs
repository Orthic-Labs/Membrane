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
    let mut keep = vec![false; lines.len()];
    let signal = lines
        .iter()
        .map(|(_, _, line)| is_signal(kind, line))
        .collect::<Vec<_>>();
    let signal_line_count = signal.iter().filter(|value| **value).count();
    for (index, _) in lines.iter().enumerate() {
        if signal[index] && keep.iter().filter(|value| **value).count() < max_lines {
            keep[index] = true;
        }
    }
    {
        let remaining = max_lines.saturating_sub(keep.iter().filter(|value| **value).count());
        for index in 0..remaining.div_ceil(2).min(lines.len()) {
            keep[index] = true;
        }
        for index in lines.len().saturating_sub(remaining / 2)..lines.len() {
            keep[index] = true;
        }
    }
    let kept_spans = spans_for(&lines, &keep, true);
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
        protected_spans: kept_spans.clone(),
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
