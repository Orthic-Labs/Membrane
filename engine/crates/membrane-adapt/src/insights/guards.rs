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
    tool_relay: Regex,
}

fn patterns() -> &'static GuardPatterns {
    static P: OnceLock<GuardPatterns> = OnceLock::new();
    P.get_or_init(|| GuardPatterns {
        hypothetical: re(
            r"(?im)\b(?:hypothetically|hypothetical|suppose|supposing|imagine|for example|e\.g\.,?|what if|if we (?:were to|had)|if it (?:were|had)|in theory)\b|(?:^|[.!?;:\n]\s*)if\b",
        ),
        negation_prefixes: re(
            r"(?im)\b(?:not|never|no|without|cannot|can't|couldn't|isn't|aren't|wasn't|weren't|hasn't|haven't|hadn't|unverified|unvalidated|untested)\b",
        ),
        clause_boundary: re(r"[.!?;\n]|\b(?:but|however|although|yet)\b"),
        tool_relay: re(
            r"(?im)\b(?:log|output|result|tool|command|test(?:s)?)\s+(?:says?|reports?|reported|returned|shows?)\b|\baccording\s+to\s+(?:the\s+)?(?:log|output|result|tool|command|test(?:s)?)\b|\b(?:based|per)\s+on\s+(?:the\s+)?(?:log|output|result|tool|command|test(?:s)?)\b",
        ),
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
    // Any balanced quotation-bearing span is non-authoritative. Suppressing
    // its whole diagnostic event is intentionally precision-first: quoted
    // text must never seed detector state through framing-length tricks.
    let mut inside = false;
    let mut delimiters = 0usize;
    let mut inside_alnum = 0usize;
    for (index, c) in text.char_indices() {
        if is_quote_delimiter(text, index, c) {
            delimiters += 1;
            inside = !inside;
            continue;
        }
        if c.is_alphanumeric() && inside {
            inside_alnum += 1;
        }
    }
    delimiters >= 2 && !inside && inside_alnum > 0
}

fn is_quote_delimiter(text: &str, index: usize, c: char) -> bool {
    if !matches!(c, '\'' | '\u{2018}' | '\u{2019}') {
        return matches!(c, '"' | '`' | '\u{201C}' | '\u{201D}');
    }
    let previous = text[..index].chars().next_back();
    let next = text[index + c.len_utf8()..].chars().next();
    !(previous.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric))
}

/// Hypothetical narration: the speaker is imagining or exemplifying, not
/// reporting an actual failure.
pub fn contains_hypothetical_narration(text: &str) -> bool {
    patterns().hypothetical.is_match(text)
}

/// True when assistant text explicitly attributes a verification claim to an
/// observable tool/log result. This only prevents an unsupported-claim
/// Insight; tool text never becomes durable authority or a Taste candidate.
pub fn is_tool_relayed_verification(text: &str) -> bool {
    patterns().tool_relay.is_match(text)
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
            let prefix = bounded_prefix(text, *start, 80);
            let last_clause = p.clause_boundary.split(prefix).last().unwrap_or("");
            !p.negation_prefixes.is_match(last_clause)
        })
        .collect()
}

/// Return at most `max_bytes` of the text immediately preceding `end`.
///
/// Byte bounds are used to keep guard work bounded, but slicing must still
/// happen on UTF-8 scalar boundaries. Moving the start forward (rather than
/// backward) preserves the byte cap when the requested bound falls inside a
/// multibyte character.
fn bounded_prefix(text: &str, end: usize, max_bytes: usize) -> &str {
    let mut end = end.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    let mut start = end.saturating_sub(max_bytes);
    while start < end && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..end]
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
        assert!(matches!(
            guard_user_span(&ev),
            SpanGuard::Suppress("tool-carried")
        ));
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
        assert!(matches!(
            guard_user_span(&ev),
            SpanGuard::Suppress("quoted")
        ));
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
        assert!(matches!(
            guard_user_span(&ev),
            SpanGuard::Suppress("hypothetical")
        ));
        assert!(contains_hypothetical_narration(
            "If the deploy had gone smoothly, I would call it done."
        ));
        assert!(contains_hypothetical_narration(
            "If everything had gone right, all tests would pass."
        ));
        assert!(contains_hypothetical_narration(
            "If deployment passes, I'll call it verified."
        ));
        assert!(contains_hypothetical_narration(
            "Context: If deployment passes, I'll call it verified."
        ));
    }

    #[test]
    fn negated_claims_are_not_positive() {
        assert!(!has_positive_verification_claim("I verified the fix works").is_empty());
        assert!(has_positive_verification_claim("It is not verified yet").is_empty());
        assert!(has_positive_verification_claim("couldn't confirm; untested").is_empty());
        assert!(!has_positive_verification_claim("it failed but now verified").is_empty());
    }

    #[test]
    fn apostrophes_in_contractions_are_not_quote_delimiters() {
        assert!(!is_mostly_quoted(
            "Actually it's still broken, that's not right."
        ));
        assert!(is_mostly_quoted(
            r#"the log said "verified and confirmed passing green done""#
        ));
        assert!(is_mostly_quoted(
            r#"The documentation says "this is bullshit and still broken"."#
        ));
        assert!(is_mostly_quoted(
            r#"They told me "this is such bullshit, fix it now" but that is not how I would put it."#
        ));
        assert!(is_mostly_quoted(
            r#"The ticket says "you forgot to add the rate limiter" but I am just quoting it for context."#
        ));
        assert!(is_mostly_quoted(
            r#"I told you "verified and passing" in the earlier note."#
        ));
    }

    #[test]
    fn tool_relay_framing_is_detected_without_authorizing_tool_text() {
        assert!(is_tool_relayed_verification(
            "The log says the deploy is verified and all set."
        ));
        assert!(!is_tool_relayed_verification(
            "I verified the deploy and fixed it."
        ));
    }

    #[test]
    fn verification_prefix_bound_is_utf8_safe() {
        let text = format!("{}—{}verified", "a".repeat(15), " ".repeat(78));
        assert_eq!(has_positive_verification_claim(&text), vec![(96, 104)]);
    }
}
