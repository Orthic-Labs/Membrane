//! Deterministic failure-episode detectors.
//!
//! Each detector is a pure function over the ordered event list returning
//! zero or more `FailureEpisodeV1`s with stable IDs. Detectors are
//! independent and idempotent; invocation order does not matter.
//!
//! Families 1–19 port the Python oracle's deterministic detectors; families
//! 20+ add the canonical required behavioral classes.

use regex::Regex;
use std::sync::OnceLock;

use super::guards::{
    contains_hypothetical_narration, guard_user_span, has_positive_verification_claim,
    is_mostly_quoted, is_tool_relayed_verification,
};
use super::{EventKind, FailureEpisodeV1, Severity, TranscriptEventV1, UserDisposition};

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static pattern compiles")
}

struct DetectorPatterns {
    correction: Regex,
    frustration: Regex,
    profanity: Regex,
    not_found: Regex,
    broad_search: Regex,
    wrong_target: Regex,
    silent_narrow: Regex,
    plan_change: Regex,
    omitted_req: Regex,
    tautological_test: Regex,
    guard_fire: Regex,
    postmortem: Regex,
    degraded: Regex,
    forge_open: Regex,
    forge_close: Regex,
    overengineering: Regex,
    abstraction: Regex,
    dependency: Regex,
    churn: Regex,
    planning_loop: Regex,
    noncompliance: Regex,
    model_gotcha: Regex,
    repeated_redesign: Regex,
    stale_terminology: Regex,
    assistant_overengineering: Regex,
    assistant_abstraction: Regex,
    assistant_scope_expansion: Regex,
    assistant_success: Regex,
    actual_test_result: Regex,
    weak_verification_tool: Regex,
    post_completion_contradiction: Regex,
    repeated_correction: Regex,
}

fn patterns() -> &'static DetectorPatterns {
    static P: OnceLock<DetectorPatterns> = OnceLock::new();
    P.get_or_init(|| DetectorPatterns {
        correction: re(
            r"(?im)\b(?:failed|broken|wrong|missing|not\s+fixed|still\s+fails?|actually\s+(?:it|that|this)|you\s+(?:missed|broke|skipped)|that'?s?\s+not\s+(?:what|right))\b",
        ),
        frustration: re(
            r"(?im)\b(?:frustrat\w*|annoying|annoyed|come\s+on|ugh|argh|sigh|tired\s+of|how\s+(?:many\s+times|long)|still\s+not|(?:second|third|fourth)\s+time|wrong\s+again)\b",
        ),
        profanity: re(
            r"(?im)\b(?:fuck(?:ing|ed|s)?|wtf|ffs|bullshit|shit(?:ty)?|goddamn(?:it)?|damn(?:it)?|dammit|pissed(?:\s+off)?)\b",
        ),
        not_found: re(
            r"(?im)\b(?:ENOENT|no\s+such\s+file|not\s+found|doesn'?t\s+exist|could\s+not\s+(?:find|locate))\b",
        ),
        broad_search: re(
            r"(?im)\b(?:search(?:ing)?\s+(?:the\s+)?(?:whole|entire|all)\s+(?:repo|codebase|workspace)|grep\s+-r\s+\.|grep\s+-R\b|grep\s+--recursive|rg\s+--hidden)\b",
        ),
        wrong_target: re(
            r"(?im)\b(?:wrong\s+(?:repo|repository|directory|workspace|package|module)|that'?s?\s+in\s+(?:a\s+)?different\s+(?:repo|project)|you'?re\s+in\s+the\s+wrong|not\s+(?:the|this)\s+repo)\b",
        ),
        silent_narrow: re(
            r"(?im)\b(?:just\s+(?:focusing|concentrating)\s+on|limiting\s+(?:scope|the\s+scope)\s+to|i\s+(?:will\s+)?just\s+focus\s+on|i'?ll\s+(?:just|only)\s+(?:do|handle|touch|fix))\b",
        ),
        plan_change: re(
            r"(?im)\b(?:changing\s+the\s+plan|new\s+plan|revised\s+plan|pivot(?:ing)?\s+to|switching\s+(?:to|toward)|forget\s+(?:the|that)\s+(?:plan|approach))\b",
        ),
        omitted_req: re(
            r"(?im)\b(?:you\s+(?:forgot|missed|skipped|left\s+out|ignored)|that'?s?\s+not\s+what\s+i\s+asked|i\s+(?:also|explicitly)\s+asked)\b",
        ),
        tautological_test: re(
            r"(?im)\b(?:assert\s+True\b|expect\s*\(\s*true\s*\)\.toBe\s*\(\s*true\s*\)|@unittest\.skip|#\s*always\s+pass)\b",
        ),
        guard_fire: re(
            r"(?im)\b(?:forbidden\s+scope|scope\s+violation|guard\s+(?:firing|fired|hit|triggered|violat\w*)|admission\s+refused|refus(?:ing|ed)\s+to\s+(?:proceed|apply|write))\b",
        ),
        postmortem: re(
            r"(?im)\b(?:post-?mortem|why\s+did\s+(?:you|this)\s+miss|what\s+went\s+wrong|root\s+cause\s+analysis)\b",
        ),
        degraded: re(
            r"(?im)\b(?:degraded|unavailable|stale[\s-]?cache|fallback\s+(?:mode|provider)|circuit[\s-]?broken|using\s+(?:cache|stale)\s+(?:response|value))\b",
        ),
        forge_open: re(r"(?im)\brubric\b[^.]*(?:opened|opening)|(?:opened|opening)[^.]*\brubric\b"),
        forge_close: re(r"(?im)\brubric\b[^.]*(?:closed|closing)|(?:closed|closing)[^.]*\brubric\b"),
        overengineering: re(
            r"(?im)\b(?:over[- ]?engineer\w*|way too (?:complex|elaborate)|unnecessarily complex|(?:too much|excessive) (?:abstraction|machinery|plumbing))\b",
        ),
        abstraction: re(
            r"(?im)\b(?:needless|unnecessary|pointless|gratuitous)\s+(?:abstraction|interface|wrapper|layer|indirection|trait|base ?class)\b",
        ),
        dependency: re(
            r"\b(?:unnecessary|unneeded|pointless|gratuitous)\s+dependenc(?:y|ies)|why (?:did you )?add (?:a |the )?(?:new )?dependenc(?:y|ies)",
        ),
        churn: re(
            r"(?im)\b(?:architecture\s+(?:churn|rewrite)|churn\w*[^.!?\n]{0,40}\bdesign|rewrit\w+\s+the\s+(?:whole|entire)\s+(?:architecture|design)|redesign\w*\s+(?:again|everything)|revert\w*\s+(?:your|the)\s+(?:redesign|refactor|rewrite)|(?:second|third)\s+redesign)\b",
        ),
        planning_loop: re(
            r"(?im)\b(?:(?:stop|quit)\s+planning|more\s+plans?,\s*less\s+(?:action|doing)|where\s+(?:is|are)\s+the\s+(?:actual\s+)?(?:code|changes|implementation)|zero\s+(?:edits|changes|implementation)|execute\s+now)\b",
        ),
        noncompliance: re(
            r"(?im)\b(?:ignor\w+\s+(?:my|the)\s+instruction|despite\s+(?:my|the)\s+instruction|(?:told|asked)\s+you\s+(?:twice|already|explicitly)|against\s+(?:explicit|clear)\s+instructions)\b",
        ),
        model_gotcha: re(
            r"(?im)\b(?:(?:this|that)\s+model\s+(?:keeps|always|repeatedly)\s+\w+|(?:gpt|claude|gemini|llama|mistral)[^\n]{0,80}\b(?:bug|fails?|wrong|gotcha|quirk)|(?:model|client|terminal-client)[^\n]{0,80}\bgotcha|gotcha[^\n]{0,80}\b(?:model|client|terminal-client|mistral))\b",
        ),
        repeated_redesign: re(
            r"(?im)\b(?:(?:second|third|fourth)\s+redesign|redesign\w*\s+(?:twice|again)|churn\w*[^.!?\n]{0,50}\b(?:twice|again))\b",
        ),
        stale_terminology: re(
            r"(?im)(?:\.blueprint/manifest|\bmemright\b|\bblueprint_stale\b)",
        ),
        assistant_overengineering: re(
            r"(?im)\b(?:plugin\s+architecture|strategy\s+base\s+class|abstract\s+\w*strategy|dynamic(?:ally)?\s+register\w*|over[- ]?engineer\w*)\b",
        ),
        assistant_abstraction: re(
            r"(?im)\b(?:abstract\s+\w*\s*(?:provider|source|interface)|provider\s+interface|future\w*\s+implementation|base\s+class)\b",
        ),
        assistant_scope_expansion: re(
            r"(?im)\b(?:i\s+also|also\s+(?:updated|changed|migrated|renamed|reformatted|upgraded)|while\s+(?:i\s+was\s+)?at\s+it|in\s+addition\s+i)\b",
        ),
        assistant_success: re(
            r"(?im)\b(?:done|fixed|all\s+green|all\s+set|complete(?:d)?|verified|passing|checks?\s+out|works?\s+as\s+intended|everything\s+(?:is\s+)?(?:correct|accurate))\b",
        ),
        actual_test_result: re(
            r"(?im)\b(?:tests?:\s*\d+\s+passed|\d+\s+passed(?:,\s*0\s+failed)?|test\s+result|exit\s+code[: ]+0)\b",
        ),
        weak_verification_tool: re(
            r"(?im)^\s*(?:echo|printf)\b[^\n]*(?:tests?\s+pass|all\s+green|verified)",
        ),
        post_completion_contradiction: re(
            r"(?im)\b(?:no\s+\w+\s+(?:route|endpoint|implementation)[^.!?\n]{0,40}\b(?:diff|code)|nothing\s+(?:was\s+)?(?:implemented|wired)|not\s+(?:actually\s+)?(?:done|implemented)|isn'?t\s+(?:even\s+)?implemented)\b",
        ),
        repeated_correction: re(
            r"(?im)\b(?:again\s*[:,-]|second\s+time|third\s+time|how\s+many\s+times)\b",
        ),
    })
}

fn user_events(events: &[TranscriptEventV1]) -> Vec<(usize, &TranscriptEventV1)> {
    events
        .iter()
        .enumerate()
        .filter(|(_, e)| guard_user_span(e).is_pass())
        .collect()
}

fn observable(event: &TranscriptEventV1) -> bool {
    event.evidence_eligible
}

fn allowed_assistant_claim(event: &TranscriptEventV1) -> bool {
    observable(event)
        && event.kind == EventKind::AssistantMessage
        && !is_mostly_quoted(&event.text)
        && !contains_hypothetical_narration(&event.text)
}

fn episode_from(
    detector: &str,
    severity: Severity,
    confidence: f64,
    signature: &str,
    observed: &str,
    events: &[TranscriptEventV1],
    idx: usize,
) -> FailureEpisodeV1 {
    let expectation = FailureEpisodeV1::nearest_user_text(events, idx);
    FailureEpisodeV1::new(
        detector,
        severity,
        confidence,
        signature,
        observed,
        &expectation,
        &[&events[idx]],
    )
}

// ---- repeated_ask ----------------------------------------------------------

/// Normalize a user request into its recurrence signature. Two asks with the
/// same signature recur deterministically across sessions.
pub fn repeated_ask_signature(text: &str) -> String {
    crate::canonical::normalize_text(text)
        .split_whitespace()
        .filter(|w| !matches!(*w, "please" | "again" | "just" | "still" | "yet"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn detect_repeated_ask(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut out = Vec::new();
    let mut seen: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (idx, ev) in user_events(events) {
        let len = ev.text.chars().count();
        if !(8..=300).contains(&len) {
            continue;
        }
        seen.entry(repeated_ask_signature(&ev.text))
            .or_default()
            .push(idx);
    }
    for (signature, indexes) in seen {
        if indexes.len() < 2 {
            continue;
        }
        // One episode per occurrence so downstream recurrence grouping sees
        // the full multiplicity within a single stream.
        for idx in indexes {
            let mut ep = episode_from(
                "repeated_ask",
                Severity::Medium,
                0.85,
                &signature,
                "user repeated an identical request after it was already handled",
                events,
                idx,
            );
            ep.user_disposition = UserDisposition::Repeated;
            out.push(ep);
        }
    }
    out
}

// ---- frustration / swearing -------------------------------------------------

pub fn detect_visible_frustration(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    collect_signal(
        events,
        "visible_frustration",
        Severity::Medium,
        0.7,
        &patterns().frustration,
    )
    .into_iter()
    .filter(|episode| {
        !episode.evidence.iter().any(|span| {
            let text = span.text.to_lowercase();
            text.contains("not frustrated") || text.contains("not frustrating")
        })
    })
    .collect()
}

pub fn detect_user_swearing(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    collect_signal(
        events,
        "user_swearing",
        Severity::High,
        0.9,
        &patterns().profanity,
    )
}

fn collect_signal(
    events: &[TranscriptEventV1],
    detector: &str,
    severity: Severity,
    confidence: f64,
    pattern: &Regex,
) -> Vec<FailureEpisodeV1> {
    user_events(events)
        .into_iter()
        .filter(|(_, ev)| pattern.is_match(&ev.text))
        .map(|(idx, _)| {
            episode_from(
                detector,
                severity,
                confidence,
                detector,
                "frustration signal in user message",
                events,
                idx,
            )
        })
        .collect()
}

// ---- claimed_verified_then_corrected ---------------------------------------

pub fn detect_claimed_verified_then_corrected(
    events: &[TranscriptEventV1],
) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    let mut out = Vec::new();
    let assistant_claims: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            allowed_assistant_claim(e) && !has_positive_verification_claim(&e.text).is_empty()
        })
        .map(|(i, _)| i)
        .collect();
    for cidx in assistant_claims {
        if let Some(uev) = events
            .iter()
            .skip(cidx + 1)
            .find(|e| guard_user_span(e).is_pass())
        {
            if guard_user_span(uev).is_pass() && p.correction.is_match(&uev.text) {
                let mut ep = FailureEpisodeV1::new(
                    "claimed_verified_then_corrected",
                    Severity::High,
                    0.9,
                    "claimed-then-corrected",
                    "assistant claimed verified/completed; user immediately reported it broken",
                    "",
                    &[&events[cidx], uev],
                );
                ep.user_disposition = UserDisposition::Escalated;
                out.push(ep);
            }
        }
    }
    out
}

// ---- ignored_tool_failure ---------------------------------------------------

/// An assistant success claim AFTER a failed tool result with no corrective
/// tool call in between.
pub fn detect_ignored_tool_failure(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut out = Vec::new();
    let mut last_failed: Option<usize> = None;
    for idx in 0..events.len() {
        if !observable(&events[idx]) {
            continue;
        }
        match events[idx].kind {
            EventKind::ToolResult => {
                last_failed = if looks_like_failure(&events[idx].text) {
                    Some(idx)
                } else {
                    None
                };
            }
            EventKind::ToolCall => last_failed = None,
            EventKind::AssistantMessage => {
                if let Some(f) = last_failed {
                    if allowed_assistant_claim(&events[idx])
                        && !has_positive_verification_claim(&events[idx].text).is_empty()
                    {
                        let mut ep = FailureEpisodeV1::new(
                            "ignored_tool_failure",
                            Severity::Critical,
                            0.92,
                            "ignored-tool-failure",
                            "assistant claimed success while the previous tool result was a failure",
                            "",
                            &[&events[f], &events[idx]],
                        );
                        ep.likely_mechanism =
                            "candidate: tool result not consulted before claim".into();
                        out.push(ep);
                        last_failed = None;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn looks_like_failure(text: &str) -> bool {
    static FAILURE: OnceLock<Regex> = OnceLock::new();
    FAILURE
        .get_or_init(|| re(r"(?im)\b(?:error|failed|failure|exit code[: ]+[1-9]|panic|exception|traceback|denied|refused)\b"))
        .is_match(text)
        && !Regex::new(r"(?im)\b0\s+(?:failed|failures?|errors?)\b")
            .expect("static pattern compiles")
            .is_match(text)
}

// ---- degraded_provider_treated_as_success ----------------------------------

pub fn detect_degraded_provider_treated_as_success(
    events: &[TranscriptEventV1],
) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    let mut out = Vec::new();
    for idx in 1..events.len() {
        let prev = &events[idx - 1];
        let cur = &events[idx];
        if observable(prev)
            && observable(cur)
            && prev.kind == EventKind::ToolResult
            && p.degraded.is_match(&prev.text)
            && cur.kind == EventKind::AssistantMessage
            && allowed_assistant_claim(cur)
            && patterns().assistant_success.is_match(&cur.text)
            && !Regex::new(r"(?im)\b(?:stale|fallback|unverified|pending|unavailable)\b")
                .expect("static pattern compiles")
                .is_match(&cur.text)
        {
            out.push(FailureEpisodeV1::new(
                "degraded_provider_treated_as_success",
                Severity::Medium,
                0.75,
                "degraded-as-success",
                "degraded/fallback provider output was presented as a completed result",
                "",
                &[prev, cur],
            ));
        }
    }
    out
}

// ---- false_not_found --------------------------------------------------------

pub fn detect_false_not_found_after_search(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    let mut out = Vec::new();
    let mut pending: Option<usize> = None;
    for idx in 0..events.len() {
        if !observable(&events[idx]) {
            continue;
        }
        match events[idx].kind {
            EventKind::ToolResult => {
                if p.not_found.is_match(&events[idx].text) {
                    pending = Some(idx);
                } else if let Some(nf) = pending {
                    if !looks_like_failure(&events[idx].text) {
                        out.push(FailureEpisodeV1::new(
                            "false_not_found",
                            Severity::Medium,
                            0.8,
                            "false-not-found",
                            "a later successful read contradicted an earlier not-found result",
                            "",
                            &[&events[nf], &events[idx]],
                        ));
                    }
                    pending = None;
                }
            }
            EventKind::UserMessage => {
                if !guard_user_span(&events[idx]).is_pass() {
                    continue;
                }
                if let Some(nf) = pending {
                    let lower = events[idx].text.to_lowercase();
                    if lower.contains("exists")
                        || lower.contains("it is there")
                        || lower.contains("it's there")
                    {
                        out.push(FailureEpisodeV1::new(
                            "false_not_found",
                            Severity::Medium,
                            0.7,
                            "false-not-found",
                            "tool reported not-found; user contradicted with evidence the target exists",
                            "",
                            &[&events[nf], &events[idx]],
                        ));
                        pending = None;
                    } else if lower.contains("deleted") || lower.contains("nothing to read") {
                        pending = None;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// ---- simple user-signal families --------------------------------------------

macro_rules! user_signal_detector {
    ($name:ident, $slug:expr, $sev:expr, $conf:expr, $field:ident, $observed:expr) => {
        pub fn $name(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
            let p = patterns();
            user_events(events)
                .into_iter()
                .filter(|(_, ev)| p.$field.is_match(&ev.text))
                .map(|(idx, _)| episode_from($slug, $sev, $conf, $slug, $observed, events, idx))
                .collect()
        }
    };
}

user_signal_detector!(
    detect_wrong_repo_or_subsystem,
    "wrong_repo_or_subsystem",
    Severity::Medium,
    0.75,
    wrong_target,
    "wrong target surface"
);
user_signal_detector!(
    detect_omitted_requirement,
    "omitted_requirement",
    Severity::High,
    0.85,
    omitted_req,
    "omitted requirement"
);
user_signal_detector!(
    detect_planning_instead_of_executing,
    "planning_instead_of_executing",
    Severity::Medium,
    0.7,
    planning_loop,
    "planning instead of executing"
);

fn assistant_events(
    events: &[TranscriptEventV1],
) -> impl Iterator<Item = (usize, &TranscriptEventV1)> {
    events.iter().enumerate().filter(|(_, event)| {
        allowed_assistant_claim(event) && !is_tool_relayed_verification(&event.text)
    })
}

fn is_historical_or_negated(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("historical note")
        || lower.contains("archived plan")
        || lower.contains("last quarter")
        || lower.contains("for context")
        || lower.contains("reviewing my earlier message")
        || lower.contains("would introduce")
        || lower.contains("will not")
        || lower.contains("not client-specific")
        || lower.contains("not model-specific")
        || lower.contains("no redesign")
}

pub fn detect_unproductive_broad_searching(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let matches: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            observable(event)
                && event.kind == EventKind::ToolCall
                && patterns().broad_search.is_match(&event.text)
        })
        .map(|(idx, _)| idx)
        .collect();
    if matches.len() < 2 {
        return vec![];
    }
    let refs: Vec<&TranscriptEventV1> = matches.iter().map(|idx| &events[*idx]).collect();
    vec![FailureEpisodeV1::new(
        "unproductive_broad_searching",
        Severity::Low,
        0.85,
        "repeated-broad-search",
        "multiple unbounded repository searches produced excessive output",
        "",
        &refs,
    )]
}

pub fn detect_silent_scope_narrowing(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    assistant_events(events)
        .filter(|(_, event)| {
            patterns().silent_narrow.is_match(&event.text)
                && !Regex::new(r"(?im)\b(?:then|afterward|same pass|remaining|all three)\b")
                    .expect("static pattern compiles")
                    .is_match(&event.text)
        })
        .map(|(idx, _)| {
            episode_from(
                "silent_scope_narrowing",
                Severity::Medium,
                0.8,
                "silent-scope-narrowing",
                "assistant silently narrowed requested scope",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_unaccepted_plan_change(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut out = Vec::new();
    for (idx, event) in assistant_events(events) {
        if !patterns().plan_change.is_match(&event.text) {
            continue;
        }
        if let Some((user_idx, _)) = events
            .iter()
            .enumerate()
            .skip(idx + 1)
            .find(|(_, candidate)| {
                guard_user_span(candidate).is_pass()
                    && Regex::new(r"(?im)\b(?:why\s+did\s+you\s+change|i\s+asked[^.!?\n]{0,50}\bnot\s+a|not\s+(?:a\s+)?rewrite)\b")
                        .expect("static pattern compiles")
                        .is_match(&candidate.text)
            })
        {
            out.push(FailureEpisodeV1::new(
                "unaccepted_plan_change",
                Severity::Medium,
                0.85,
                "unaccepted-plan-change",
                "assistant changed approach without acceptance",
                "",
                &[event, &events[user_idx]],
            ));
        }
    }
    out
}

pub fn detect_tests_that_cannot_fail(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            observable(event)
                && event.kind == EventKind::ToolResult
                && patterns().tautological_test.is_match(&event.text)
        })
        .filter(|(_, event)| !event.text.contains("skipIf"))
        .map(|(idx, _)| {
            episode_from(
                "tests_that_cannot_fail",
                Severity::High,
                0.95,
                "tautological-test",
                "test passes by construction",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_guard_firings(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            observable(event)
                && event.kind == EventKind::ToolResult
                && patterns().guard_fire.is_match(&event.text)
        })
        .map(|(idx, _)| {
            episode_from(
                "guard_firings",
                Severity::Low,
                0.9,
                "guard-firing",
                "runtime guard refused an operation",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_postmortem_ask(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    user_events(events)
        .into_iter()
        .filter(|(_, event)| patterns().postmortem.is_match(&event.text))
        .map(|(idx, _)| {
            episode_from(
                "user_asks_why_missed_or_postmortem",
                Severity::Medium,
                0.85,
                "postmortem-request",
                "user requested explanation for a miss",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_overengineering_family(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    assistant_events(events)
        .filter(|(_, event)| {
            (patterns().overengineering.is_match(&event.text)
                || patterns().assistant_overengineering.is_match(&event.text))
                && !is_historical_or_negated(&event.text)
        })
        .map(|(idx, _)| {
            episode_from(
                "overengineering",
                Severity::Medium,
                0.8,
                "overengineering",
                "solution introduced disproportionate machinery",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_architecture_churn_core(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    user_events(events)
        .into_iter()
        .filter(|(_, event)| {
            patterns().churn.is_match(&event.text) && !is_historical_or_negated(&event.text)
        })
        .map(|(idx, _)| {
            episode_from(
                "architecture_churn",
                Severity::High,
                0.85,
                "architecture-churn",
                "architecture changed repeatedly without new evidence",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_scope_expansion_without_request(
    events: &[TranscriptEventV1],
) -> Vec<FailureEpisodeV1> {
    assistant_events(events)
        .filter(|(_, event)| patterns().assistant_scope_expansion.is_match(&event.text))
        .map(|(idx, _)| {
            episode_from(
                "scope_expansion_without_request",
                Severity::High,
                0.9,
                "unrequested-scope-expansion",
                "assistant added unrelated work",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_repeated_scope_expansion(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let additions: Vec<usize> = assistant_events(events)
        .filter(|(_, event)| patterns().assistant_scope_expansion.is_match(&event.text))
        .map(|(idx, _)| idx)
        .collect();
    if additions.len() < 2 {
        return vec![];
    }
    let refs: Vec<&TranscriptEventV1> = additions.iter().map(|idx| &events[*idx]).collect();
    vec![FailureEpisodeV1::new(
        "repeated_scope_expansion",
        Severity::Critical,
        0.9,
        "repeated-unrequested-scope-expansion",
        "assistant expanded scope more than once",
        "",
        &refs,
    )]
}

pub fn detect_verification_theatre(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut weak_call: Option<usize> = None;
    for (idx, event) in events.iter().enumerate() {
        if observable(event)
            && event.kind == EventKind::ToolCall
            && patterns().weak_verification_tool.is_match(&event.text)
        {
            weak_call = Some(idx);
        } else if observable(event)
            && event.kind == EventKind::ToolResult
            && patterns().actual_test_result.is_match(&event.text)
        {
            weak_call = None;
        } else if allowed_assistant_claim(event)
            && patterns().assistant_success.is_match(&event.text)
            && weak_call.is_some()
        {
            return vec![FailureEpisodeV1::new(
                "verification_theatre",
                Severity::High,
                0.95,
                "verification-theatre",
                "assistant used self-authored output as verification evidence",
                "",
                &[&events[weak_call.expect("checked")], event],
            )];
        }
    }
    vec![]
}

pub fn detect_false_completion_claim(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut claim: Option<usize> = None;
    let mut out = Vec::new();
    for (idx, event) in events.iter().enumerate() {
        if allowed_assistant_claim(event) && patterns().assistant_success.is_match(&event.text) {
            claim = Some(idx);
        } else if guard_user_span(event).is_pass()
            && patterns()
                .post_completion_contradiction
                .is_match(&event.text)
        {
            if let Some(claim_idx) = claim.take() {
                out.push(FailureEpisodeV1::new(
                    "false_completion_claim",
                    Severity::High,
                    0.95,
                    "false-completion-claim",
                    "user contradicted assistant completion claim with missing implementation evidence",
                    "",
                    &[&events[claim_idx], event],
                ));
            }
        }
    }
    out
}

pub fn detect_instruction_noncompliance(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut prohibition: Option<(usize, String)> = None;
    for (idx, event) in events.iter().enumerate() {
        if guard_user_span(event).is_pass() {
            let lower = event.text.to_lowercase();
            if lower.contains("do not touch") || lower.contains("don't touch") {
                prohibition = Some((idx, lower));
            } else if prohibition.is_some()
                && (patterns().noncompliance.is_match(&event.text)
                    || lower.contains("i told you not to touch"))
            {
                let (prohibition_idx, _) = prohibition.take().expect("checked");
                return vec![FailureEpisodeV1::new(
                    "instruction_noncompliance",
                    Severity::High,
                    0.95,
                    "explicit-instruction-noncompliance",
                    "assistant changed an explicitly prohibited surface",
                    "",
                    &[&events[prohibition_idx], event],
                )];
            }
        }
        if observable(event) && event.kind == EventKind::ToolCall {
            if let Some((prohibition_idx, text)) = &prohibition {
                if text.contains("migrations/") && event.text.contains("migrations/") {
                    return vec![FailureEpisodeV1::new(
                        "instruction_noncompliance",
                        Severity::High,
                        0.98,
                        "explicit-instruction-noncompliance",
                        "tool call targeted an explicitly prohibited path",
                        "",
                        &[&events[*prohibition_idx], event],
                    )];
                }
            }
        }
    }
    vec![]
}

// ---- canonical additions with distinct shapes -------------------------------

/// Public entry for architecture churn; `detect_repeated_redesign` escalates
/// when >=2 churn signatures appear in one session.
pub fn detect_architecture_churn(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    detect_architecture_churn_core(events)
}

pub fn detect_repeated_redesign(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    user_events(events)
        .into_iter()
        .filter(|(_, event)| {
            patterns().repeated_redesign.is_match(&event.text)
                && !is_historical_or_negated(&event.text)
        })
        .map(|(idx, _)| {
            episode_from(
                "repeated_redesign",
                Severity::Critical,
                0.9,
                "repeated-redesign",
                "design was replaced repeatedly without measurement",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_unnecessary_abstraction(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    assistant_events(events)
        .filter(|(_, ev)| {
            (p.abstraction.is_match(&ev.text) || p.assistant_abstraction.is_match(&ev.text))
                && !is_historical_or_negated(&ev.text)
        })
        .map(|(idx, _)| {
            let mut ep = episode_from(
                "unnecessary_abstraction",
                Severity::Medium,
                0.7,
                "unnecessary-abstraction",
                "unnecessary abstraction",
                events,
                idx,
            );
            ep.likely_mechanism = "candidate: abstraction added without demonstrated need".into();
            ep
        })
        .collect()
}

pub fn detect_unnecessary_dependency(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    user_events(events)
        .into_iter()
        .filter(|(_, ev)| {
            (p.dependency.is_match(&ev.text)
                || Regex::new(r"(?im)\b(?:whole|new)\s+dependenc(?:y|ies)\b")
                    .expect("static pattern compiles")
                    .is_match(&ev.text))
                && !Regex::new(r"(?im)\b(?:do\s+not|don'?t|no\s+new)\b")
                    .expect("static pattern compiles")
                    .is_match(&ev.text)
        })
        .map(|(idx, _)| {
            episode_from(
                "unnecessary_dependency",
                Severity::Medium,
                0.7,
                "unnecessary-dependency",
                "unnecessary dependency added",
                events,
                idx,
            )
        })
        .collect()
}

/// Model- vs client/tool-specific gotcha attribution: the same normalized
/// complaint text is classified by which surface the user names.
pub fn detect_model_or_client_specific_gotcha(
    events: &[TranscriptEventV1],
) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    static CLIENT: OnceLock<Regex> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        re(r"(?im)\b(?:vscode|vim|jetbrains|terminal(?:-client)?|iterm|warp|xcode|pi|cli)\b")
    });
    assistant_events(events)
        .filter(|(_, ev)| p.model_gotcha.is_match(&ev.text) && !is_historical_or_negated(&ev.text))
        .map(|(idx, ev)| {
            let slug = if client.is_match(&ev.text) {
                "client_or_tool_specific_gotcha"
            } else {
                "model_specific_gotcha"
            };
            episode_from(
                slug,
                Severity::Low,
                0.6,
                slug,
                "surface-specific gotcha signal",
                events,
                idx,
            )
        })
        .collect()
}

/// Repeated user correction on the same theme: corrections sharing a 3-word
/// normalized prefix recur into a longitudinal signal.
pub fn detect_repeated_user_correction_same_theme(
    events: &[TranscriptEventV1],
) -> Vec<FailureEpisodeV1> {
    user_events(events)
        .into_iter()
        .filter(|(_, event)| patterns().repeated_correction.is_match(&event.text))
        .map(|(idx, _)| {
            episode_from(
                "repeated_user_correction_same_theme",
                Severity::High,
                0.9,
                "repeated-user-correction",
                "user explicitly marked a correction as repeated",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_verification_claim_without_tool_evidence(
    events: &[TranscriptEventV1],
) -> Vec<FailureEpisodeV1> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| allowed_assistant_claim(event))
        .filter(|(idx, event)| {
            !has_positive_verification_claim(&event.text).is_empty()
                && !is_historical_or_negated(&event.text)
                && !events[..*idx].iter().any(|prior| {
                    observable(prior)
                        && prior.session_id == event.session_id
                        && prior.kind == EventKind::ToolResult
                        && event.call_id.is_some()
                        && prior.call_id == event.call_id
                        && (patterns().actual_test_result.is_match(&prior.text)
                            || (is_tool_relayed_verification(&event.text)
                                && !has_positive_verification_claim(&prior.text).is_empty()))
                })
        })
        .map(|(idx, _)| {
            episode_from(
                "verification_claim_without_tool_evidence",
                Severity::High,
                0.9,
                "unsupported-verification-claim",
                "assistant claimed verification without tool evidence",
                events,
                idx,
            )
        })
        .collect()
}

pub fn detect_cross_agent_repeats(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut seen: std::collections::BTreeMap<String, (usize, String)> =
        std::collections::BTreeMap::new();
    for (idx, event) in assistant_events(events) {
        let signature = crate::canonical::normalize_text(&event.text);
        if signature.len() < 8 {
            continue;
        }
        if let Some((prior_idx, prior_session)) = seen.get(&signature) {
            if prior_session != &event.session_id {
                return vec![FailureEpisodeV1::new(
                    "cross_agent_repeats",
                    Severity::Medium,
                    0.95,
                    &signature,
                    "same assistant response recurred across sessions",
                    "",
                    &[&events[*prior_idx], event],
                )];
            }
        } else {
            seen.insert(signature, (idx, event.session_id.clone()));
        }
    }
    vec![]
}

pub fn detect_stale_terminology_surfacing(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    assistant_events(events)
        .filter(|(_, event)| {
            patterns().stale_terminology.is_match(&event.text)
                && !is_historical_or_negated(&event.text)
                && !event.text.to_lowercase().contains("retired")
        })
        .map(|(idx, _)| {
            episode_from(
                "stale_terminology_surfacing",
                Severity::Low,
                0.95,
                "stale-terminology",
                "retired product terminology surfaced as current guidance",
                events,
                idx,
            )
        })
        .collect()
}

// ---- forge_opened_never_closed ----------------------------------------------

pub fn detect_forge_opened_never_closed(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let p = patterns();
    let mut opened = std::collections::BTreeMap::<&str, usize>::new();
    for (idx, ev) in events.iter().enumerate() {
        if allowed_assistant_claim(ev)
            && !is_tool_relayed_verification(&ev.text)
            && p.forge_open.is_match(&ev.text)
        {
            opened.entry(&ev.session_id).or_insert(idx);
        } else if allowed_assistant_claim(ev)
            && !is_tool_relayed_verification(&ev.text)
            && p.forge_close.is_match(&ev.text)
        {
            opened.remove(ev.session_id.as_str());
        }
    }
    let mut open_indices: Vec<usize> = opened.values().copied().collect();
    open_indices.sort_unstable();
    open_indices
        .into_iter()
        .map(|o| {
            episode_from(
                "forge_opened_never_closed",
                Severity::Low,
                0.7,
                "forge-open-never-closed",
                "rubric/work item opened but never closed before end of session",
                events,
                o,
            )
        })
        .collect()
}

// ---- orchestration -----------------------------------------------------------

/// Run every detector family over the event stream and return all episodes.
/// Deterministic ordering: severity descending, then episode ID ascending.
pub fn run_all_detectors(events: &[TranscriptEventV1]) -> Vec<FailureEpisodeV1> {
    let mut out = Vec::new();
    out.extend(detect_visible_frustration(events));
    out.extend(detect_user_swearing(events));
    out.extend(detect_repeated_ask(events));
    out.extend(detect_verification_claim_without_tool_evidence(events));
    out.extend(detect_claimed_verified_then_corrected(events));
    out.extend(detect_ignored_tool_failure(events));
    out.extend(detect_degraded_provider_treated_as_success(events));
    out.extend(detect_false_not_found_after_search(events));
    out.extend(detect_unproductive_broad_searching(events));
    out.extend(detect_wrong_repo_or_subsystem(events));
    out.extend(detect_stale_terminology_surfacing(events));
    out.extend(detect_silent_scope_narrowing(events));
    out.extend(detect_omitted_requirement(events));
    out.extend(detect_unaccepted_plan_change(events));
    out.extend(detect_tests_that_cannot_fail(events));
    out.extend(detect_guard_firings(events));
    out.extend(detect_postmortem_ask(events));
    out.extend(detect_verification_theatre(events));
    out.extend(detect_overengineering_family(events));
    out.extend(detect_unnecessary_abstraction(events));
    out.extend(detect_unnecessary_dependency(events));
    out.extend(detect_architecture_churn(events));
    out.extend(detect_repeated_redesign(events));
    out.extend(detect_planning_instead_of_executing(events));
    out.extend(detect_scope_expansion_without_request(events));
    out.extend(detect_repeated_scope_expansion(events));
    out.extend(detect_false_completion_claim(events));
    out.extend(detect_instruction_noncompliance(events));
    out.extend(detect_model_or_client_specific_gotcha(events));
    out.extend(detect_cross_agent_repeats(events));
    out.extend(detect_repeated_user_correction_same_theme(events));
    out.extend(detect_forge_opened_never_closed(events));
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.episode_id.cmp(&b.episode_id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        kind: EventKind,
        provenance: &str,
        text: &str,
        evidence_eligible: bool,
    ) -> TranscriptEventV1 {
        TranscriptEventV1 {
            event_id: format!("{}-{}", provenance, text.len()),
            session_id: "s1".into(),
            host: "test".into(),
            provenance: provenance.into(),
            kind,
            text: text.into(),
            timestamp: Some("2026-08-26T00:00:00Z".into()),
            byte_start: 0,
            byte_end: text.len() as i64,
            call_id: None,
            occurrence: 0,
            evidence_eligible,
        }
    }

    #[test]
    fn quoted_and_hypothetical_assistant_claims_are_suppressed() {
        for text in [
            r#"the log says "verified and fixed all tests""#,
            "hypothetically, I verified and fixed all tests",
            "If the deploy had gone smoothly, it would be verified.",
            "If everything had gone right, all tests would be passing.",
            r#"The report says "I also updated the changelog while I was at it.""#,
        ] {
            let events = vec![
                event(EventKind::AssistantMessage, "assistant", text, true),
                event(
                    EventKind::UserMessage,
                    "external_user",
                    "it is still broken and missing",
                    true,
                ),
            ];
            assert!(detect_claimed_verified_then_corrected(&events).is_empty());
            assert!(detect_verification_claim_without_tool_evidence(&events).is_empty());
            assert!(detect_false_completion_claim(&events).is_empty());
            assert!(detect_scope_expansion_without_request(&events).is_empty());
        }
    }

    #[test]
    fn forge_state_requires_unquoted_assistant_observation() {
        for mut ev in [
            event(EventKind::ToolResult, "tool", "Opened the rubric.", true),
            event(
                EventKind::AssistantMessage,
                "assistant",
                r#"The report says "Opened the rubric.""#,
                true,
            ),
            event(
                EventKind::AssistantMessage,
                "assistant",
                "If we had opened the rubric, it would remain active.",
                true,
            ),
        ] {
            ev.call_id = Some("call-forge".into());
            assert!(detect_forge_opened_never_closed(&[ev]).is_empty());
        }

        let cross_session = vec![
            event(
                EventKind::AssistantMessage,
                "assistant",
                "Opened the rubric for session one.",
                true,
            ),
            {
                let mut close = event(
                    EventKind::AssistantMessage,
                    "assistant",
                    "Closed the rubric for session two.",
                    true,
                );
                close.session_id = "s2".into();
                close
            },
        ];
        assert_eq!(detect_forge_opened_never_closed(&cross_session).len(), 1);
    }

    #[test]
    fn ineligible_events_cannot_seed_or_advance_detector_state() {
        let events = vec![
            event(EventKind::ToolResult, "tool", "error: tests failed", false),
            event(EventKind::AssistantMessage, "assistant", "done", true),
        ];
        assert!(detect_ignored_tool_failure(&events).is_empty());

        let events = vec![
            event(
                EventKind::ToolResult,
                "tool",
                "degraded fallback provider",
                false,
            ),
            event(EventKind::AssistantMessage, "assistant", "done", true),
        ];
        assert!(detect_degraded_provider_treated_as_success(&events).is_empty());

        let events = vec![
            event(EventKind::ToolCall, "tool", "echo tests pass", false),
            event(EventKind::AssistantMessage, "assistant", "done", true),
        ];
        assert!(detect_verification_theatre(&events).is_empty());
    }

    #[test]
    fn false_not_found_ignores_quoted_and_hypothetical_user_text() {
        for text in [
            r#"the report says "it exists""#,
            "hypothetically, if it exists, it is there",
        ] {
            let events = vec![
                event(EventKind::ToolResult, "tool", "ENOENT: not found", true),
                event(EventKind::UserMessage, "external_user", text, true),
            ];
            assert!(detect_false_not_found_after_search(&events).is_empty());
        }
    }

    #[test]
    fn eligible_tool_relay_supports_claim_without_granting_authority() {
        let mut events = vec![
            event(
                EventKind::ToolResult,
                "tool",
                "Deployment log: verified and all set, tests passing.",
                true,
            ),
            event(
                EventKind::AssistantMessage,
                "assistant",
                "The log says the deploy is verified and all set.",
                true,
            ),
        ];
        events[0].call_id = Some("call-deploy".into());
        events[1].call_id = Some("call-deploy".into());
        assert!(detect_verification_claim_without_tool_evidence(&events).is_empty());

        let mut ineligible = vec![
            event(
                EventKind::ToolResult,
                "tool",
                "Deployment log: verified and all set, tests passing.",
                false,
            ),
            event(
                EventKind::AssistantMessage,
                "assistant",
                "The log says the deploy is verified and all set.",
                true,
            ),
        ];
        ineligible[0].call_id = Some("call-deploy".into());
        ineligible[1].call_id = Some("call-deploy".into());
        assert!(!detect_verification_claim_without_tool_evidence(&ineligible).is_empty());

        let mut unrelated = events.clone();
        unrelated[0].call_id = Some("call-unrelated".into());
        assert!(!detect_verification_claim_without_tool_evidence(&unrelated).is_empty());
    }

    #[test]
    fn all_ineligible_rows_are_ignored_by_every_detector_family() {
        let events = vec![
            event(
                EventKind::UserMessage,
                "external_user",
                "please run tests again, this is frustrating",
                false,
            ),
            event(
                EventKind::AssistantMessage,
                "assistant",
                "done, I verified the fix",
                false,
            ),
            event(EventKind::ToolCall, "tool", "rg --hidden .", false),
            event(EventKind::ToolResult, "tool", "error: not found", false),
        ];
        assert!(run_all_detectors(&events).is_empty());
    }
}
