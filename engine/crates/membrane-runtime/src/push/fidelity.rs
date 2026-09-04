//! Independent span validation. Output never supplies its own source universe.
use serde::{Deserialize, Serialize};
use super::recovery::{digest, RecoveryError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Span { pub start: usize, pub end: usize }
#[derive(Debug, Clone)]
pub struct SpanMapping { pub source: Span, pub output: Span }

pub fn validate(
    original: &[u8], expected_digest: &str, output: &[u8],
    mappings: &[SpanMapping], obligations: &[Span],
) -> Result<(), RecoveryError> {
    if digest(original) != expected_digest { return Err(RecoveryError::Corrupt); }
    let mut source_end = 0;
    let mut output_end = 0;
    for m in mappings {
        if m.source.start < source_end || m.output.start < output_end
            || m.source.start > m.source.end || m.output.start > m.output.end
            || original.get(m.source.start..m.source.end).is_none()
            || output.get(m.output.start..m.output.end).is_none()
            || original[m.source.start..m.source.end] != output[m.output.start..m.output.end] {
            return Err(RecoveryError::Corrupt);
        }
        source_end = m.source.end;
        output_end = m.output.end;
    }
    for span in obligations {
        if span.start >= span.end || span.end > original.len() { return Err(RecoveryError::InvalidSelector); }
        let mut covered = span.start;
        for m in mappings {
            if m.source.start <= covered && m.source.end > covered { covered = m.source.end; }
            if covered >= span.end { break; }
        }
        if covered < span.end { return Err(RecoveryError::Corrupt); }
    }
    Ok(())
}

/// Conservative source-side obligations. These do not create authority: caller
/// obligations can only add protection; repository prose stays data-only.
pub fn protected_lines(source: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut fence: Option<&str> = None;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let delimiter = if trimmed.starts_with("```") { Some("```") }
            else if trimmed.starts_with("~~~") { Some("~~~") } else { None };
        let words = line.split(|c: char| !c.is_alphanumeric() && c != '_')
            .map(str::to_ascii_lowercase).collect::<Vec<_>>();
        let critical = words.iter().any(|w| ["not", "never", "must", "shall", "cannot", "don't", "error", "failed", "failure", "panic", "warning", "exception", "decision", "approved", "rejected"].contains(&w.as_str()))
            || line.chars().any(|c| c.is_numeric()) || line.contains('`') || line.contains("::")
            || line.contains("http") || line.contains('/') || line.contains('\\');
        if fence.is_some() || delimiter.is_some() || critical {
            spans.push(Span { start: offset, end: offset + line.len() });
        }
        if let Some(d) = delimiter {
            if fence == Some(d) { fence = None; } else if fence.is_none() { fence = Some(d); }
        }
        offset += line.len();
    }
    spans
}

/// Whole-line extraction retains source occurrence identity and original CRLF.
/// No protected line is deleted to manufacture budget success.
pub fn extract_lines(source: &str, budget: usize, required: &[Span]) -> Result<(String, Vec<SpanMapping>), RecoveryError> {
    let mut obligations = protected_lines(source);
    obligations.extend_from_slice(required);
    if obligations.iter().any(|s| s.start >= s.end || s.end > source.len()) { return Err(RecoveryError::InvalidSelector); }
    let lines = super::recovery::byte_lines(source.as_bytes()).collect::<Vec<_>>();
    let keep = lines.iter().map(|(start, bytes)| obligations.iter().any(|s| *start < s.end && *start + bytes.len() > s.start)).collect::<Vec<_>>();
    let protected_bytes: usize = lines.iter().zip(&keep).filter(|(_, k)| **k).map(|((_, bytes), _)| bytes.len()).sum();
    if protected_bytes > budget { return Err(RecoveryError::Limit); }
    let mut remaining = budget - protected_bytes;
    let mut output = Vec::new();
    let mut mappings = Vec::new();
    for ((start, bytes), protected) in lines.into_iter().zip(keep) {
        if protected || bytes.len() <= remaining {
            let output_start = output.len();
            output.extend_from_slice(bytes);
            mappings.push(SpanMapping { source: Span { start, end: start + bytes.len() }, output: Span { start: output_start, end: output.len() } });
            if !protected { remaining -= bytes.len(); }
        }
    }
    validate(source.as_bytes(), &digest(source.as_bytes()), &output, &mappings, &obligations)?;
    Ok((String::from_utf8(output).map_err(|_| RecoveryError::Corrupt)?, mappings))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validator_rejects_mutation_negation_loss_and_wrong_occurrence() {
        let source = b"Do not deploy\nDo not deploy\n";
        let obligation = Span { start:14, end:28 };
        let mapping = SpanMapping { source:Span {start:0,end:14}, output:Span {start:0,end:14} };
        assert!(validate(source, &digest(source), b"Do not deploy\n", &[mapping], &[obligation]).is_err());
        let mapping = SpanMapping { source:Span {start:0,end:14}, output:Span {start:0,end:10} };
        assert!(validate(source, &digest(source), b"Do deploy\n", &[mapping], &[]).is_err());
        let mapping = SpanMapping { source:Span {start:0,end:14}, output:Span {start:0,end:14} };
        assert!(validate(source, &digest(source), b"DO NOT DEPLOY\n", &[mapping], &[]).is_err());
    }
    #[test]
    fn extraction_protects_late_failures_and_preserves_crlf() {
        let text = "ordinary prose\r\nmore prose\r\nerror: must not deploy\r\n";
        let (out, _) = extract_lines(text, 25, &[]).unwrap();
        assert_eq!(out, "error: must not deploy\r\n");
        assert!(extract_lines(text, 5, &[]).is_err());
    }
}
