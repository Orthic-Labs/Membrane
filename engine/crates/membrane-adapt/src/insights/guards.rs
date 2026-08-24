//! Deterministic false-positive guards: quoted text, tool-carried text,
//! hypothetical narration, and local negation. User-utterance-driven
//! detectors MUST apply these before firing.

use regex::Regex;
use std::sync::OnceLock;

use super::{EventKind, TranscriptEventV1};

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static pattern compiles")
}

struct GuardPatterns {
    hypothetical: Regex,
    negation_prefixes: Regex,
    clause_boundary: Regex,
    quoted: Regex,
}

fn patterns() -> &'static GuardPatterns {
    static P: OnceLock<GuardPatterns> = OnceLock::new();
    P.get_or_init(|| GuardPatterns {
        hypothetical: re(
            r"(?im)\b(?:hypothetically|hypothetical|suppose|supposing|imagine|for example|e\.g\.,?|what if|if we (?:were to|had)|if it (?:were|had)|in theory)\b",
        ),
        negation_prefixes: re(
            r"(?im)\b(?:not|never|no|without|cannot|can't|couldn't|isn't|aren't|wasn't|weren't|hasn't|haven't|hadn't|unverified|unvalidated|untested)\b",
        ),
        clause_boundary: re(r"[.!?;\n]|\b(?:but|however|although|yet)\b"),
        quoted: re(r#"["'`“”‘’][^"'`“”‘’]{4,}["'`“”‘’]"#),
    })
}

/// A span is tool-carried when it comes from a tool call/result event rather
/// than a user utterance — logs, command output, and error text must never
/// fire user-utterance detectors.
pub fn is_tool_carried(event: &TranscriptEventV1) -> bool {
    matches!(event.kind, EventKind::ToolCall | EventKind::ToolResult)
}

/// True when the dominant content of the span is quoted material (a quote of
/// someone else's words, a pasted log line, or echoed code).
pub fn is_mostly_quoted(text: &str) -> bool {
    let p = patterns();
    if !p.quoted.is_match(text) {
        return false;
    }
    // If every alphabetic run of length >= 8 sits inside quotes, treat the
    // span as quoted material. Approximation is fine for a guard: it errs
    // toward suppression, which is precision-first.
    let mut inside = false;
    let mut run = 0usize;
    let mut longest_outside = 0usize;
    for c in text.chars() {
        if matches!(c, '"' | '\'' | '`' | '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}') {
            inside = !inside;
            longest_outside = longest_outside.max(run);
            run = 0;
            continue;
        }
        let alpha = c.is_alphanumeric();
        match (inside, alpha) {
            (true, true) => {}
            (false, true) => run += 1,
            (_, false) => {
                longest_outside = if inside { longest_outside } else { longest_outside.max(run) };
                run = 0;
            }
        }
    }
    if !inside {
        longest_outside = longest_outside.max(run);
    }
    longest_outside < 12
}

/// Hypothetical narration: the speaker is imagining or exemplifying, not
/// reporting an actual failure.
pub fn contains_hypothetical_narration(text: &str) -> bool {
    patterns().hypothetical.is_match(text)
}

/// Locally-negated completion/verification phrases are NOT positive claims:
/// "not verified", "tests couldn't pass" must not fire claim detectors.
/// Mirrors the Python oracle's clause-local negation handling.
pub fn has_positive_verification_claim(text: &str) -> Vec<(usize, usize)> {
    lazy_static_verification_matches(text)
}

fn verification_word_at(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    static VERIFICATION: OnceLock<Regex> = OnceLock::new();
    let pattern = VERIFICATION.get_or_init(|| re(
        r"(?im)\b(?:verified|validated|fixed|fully\s+fixed|tested|confirmed|all\s+set|done|passing|green|works)\b",
    ));
    pattern.find_iter(text).map(|m| (m.start(), m.end()))
}

fn lazy_static_verification_matches(text: &str) -> Vec<(usize, usize)> {
    let p = patterns();
    verification_word_at(text)
        .filter(|(start, _)| {
            let prefix_start = start.saturating_sub(80);
            let prefix = &text[prefix_start..*start];
            let last_clause = p.clause_boundary.split(prefix).last().unwrap_or("");
            !p.negation_prefixes.is_match(last_clause)
        })
        .collect()
}

/// Standard pre-flight for user-utterance-driven detection.
pub enum SpanGuard {
    /// Detector may proceed on this span.
    Pass,
    /// Suppressed with reason; record for benchmark FP accounting.
    Suppress(&'static str),
}

impl SpanGuard {
    pub fn is_pass(&self) -> bool {
        matches!(self, SpanGuard::Pass)
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            SpanGuard::Pass => None,
            SpanGuard::Suppress(r) => Some(r),
        }
    }
}

pub fn guard_user_span(event: &TranscriptEventV1) -> SpanGuard {
    if !event.evidence_eligible {
        return SpanGuard::Suppress("ineligible");
    }
    if is_tool_carried(event) {
        return SpanGuard::Suppress("tool-carried");
    }
    if !event.is_user() {
        return SpanGuard::Suppress("not-external-user");
    }
    if is_mostly_quoted(&event.text) {
        return SpanGuard::Suppress("quoted");
    }
    if contains_hypothetical_narration(&event.text) {
        return SpanGuard::Suppress("hypothetical");
    }
    SpanGuard::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_events_are_suppressed() {
        let ev = TranscriptEventV1 {
            event_id: "e".into(),
            session_id: "s".into(),
            host: "h".into(),
            provenance: "tool".into(),
            kind: EventKind::ToolResult,
            text: "error: tests failed".into(),
            timestamp: None,
            byte_start: 0,
            byte_end: 10,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        };
        assert!(matches!(guard_user_span(&ev), SpanGuard::Suppress("tool-carried")));
    }

    #[test]
    fn quoted_text_is_suppressed() {
        let ev = TranscriptEventV1 {
            event_id: "e".into(),
            session_id: "s".into(),
            host: "h".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: r#"the log said "verified and confirmed passing green done""#.into(),
            timestamp: None,
            byte_start: 0,
            byte_end: 10,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        };
        assert!(matches!(guard_user_span(&ev), SpanGuard::Suppress("quoted")));
    }

    #[test]
    fn hypothetical_text_is_suppressed() {
        let ev = TranscriptEventV1 {
            event_id: "e".into(),
            session_id: "s".into(),
            host: "h".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: "suppose you had verified everything and it works, what would you do next".into(),
            timestamp: None,
            byte_start: 0,
            byte_end: 10,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        };
        assert!(matches!(guard_user_span(&ev), SpanGuard::Suppress("hypothetical")));
    }

    #[test]
    fn negated_claims_are_not_positive() {
        assert!(!has_positive_verification_claim("I verified the fix works").is_empty());
        assert!(has_positive_verification_claim("It is not verified yet").is_empty());
        assert!(has_positive_verification_claim("couldn't confirm; untested").is_empty());
        assert!(!has_positive_verification_claim("it failed but now verified").is_empty());
    }
}
