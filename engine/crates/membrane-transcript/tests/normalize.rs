//! Integration tests over committed fixtures: determinism, byte spans,
//! linkage, provenance, evidence flags, redaction, typed errors, receipts.

use std::path::PathBuf;

use membrane_transcript::{
    classify::Classification, detect_host, parse, parse_prefix_receipt, parse_source_events,
    resolve_session, SessionCandidate, TranscriptError, PARSER_VERSION,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn claude_code_fixture_normalizes_with_spans_and_linkage() {
    let path = fixture("claude_code.jsonl");
    let events = parse_source_events(&path, None).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "thinking",
            "assistant_message",
            "tool_call",
            "tool_result",
            "user_message",
            "meta"
        ]
    );

    let reasoning = events.iter().find(|e| e.kind == "thinking").unwrap();
    assert!(reasoning.private_reasoning_omitted && reasoning.meta);
    assert_eq!(reasoning.text, "private reasoning omitted");

    // Byte spans slice the original file back exactly.
    let bytes = std::fs::read(&path).unwrap();
    for ev in &events {
        let row = &bytes[ev.byte_start as usize..ev.byte_end as usize];
        assert_eq!(row.last(), Some(&b'\n'));
        assert!(std::str::from_utf8(row).is_ok());
        assert_eq!(ev.transcript_id, "claude_code");
        assert_eq!(ev.host, "claude_code");
        assert_eq!(ev.session_id, "sess-claude-1");
    }

    // Tool linkage: result points at its call; occurrences are paired.
    let call = events.iter().find(|e| e.kind == "tool_call").unwrap();
    let result = events.iter().find(|e| e.kind == "tool_result").unwrap();
    assert_eq!(call.call_id.as_deref(), Some("toolu_1"));
    // Occurrence shares one per-call_id counter across calls and results
    // (byte-faithful port of the retired normalizer): the result follows its
    // call, so it carries the next ordinal.
    assert_eq!(call.occurrence, Some(0));
    assert_eq!(result.occurrence, Some(1));
    assert_eq!(
        result.tool_call_event_id.as_deref(),
        Some(call.event_id.as_str())
    );

    // Mutation classification for the edit tool; open user request for the ask.
    assert_eq!(call.classification(), Classification::Mutation);
    let ask = events.iter().find(|e| e.kind == "user_message").unwrap();
    assert_eq!(ask.classification(), Classification::OpenUserRequest);

    // Meta event carries synthetic + meta evidence flags.
    let meta = events.iter().find(|e| e.kind == "meta").unwrap();
    assert!(meta.synthetic && meta.meta);
}

#[test]
fn codex_fixture_links_call_to_output_and_flags_failure() {
    let path = fixture("codex.jsonl");
    let events = parse_source_events(&path, Some("codex")).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["user_message", "thinking", "tool_call", "tool_result"]
    );

    let reasoning = events.iter().find(|e| e.kind == "thinking").unwrap();
    assert!(reasoning.private_reasoning_omitted && reasoning.meta);
    assert_eq!(reasoning.text, "private reasoning omitted");

    let call = events.iter().find(|e| e.kind == "tool_call").unwrap();
    let out = events.iter().find(|e| e.kind == "tool_result").unwrap();
    assert_eq!(
        out.tool_call_event_id.as_deref(),
        Some(call.event_id.as_str())
    );
    assert!(out.flags.is_error || out.classification() == Classification::UnresolvedFailure);
    assert!(out.text.contains("exit code: 101"));
}

#[test]
fn generic_pi_fixture_preserves_provenance_and_decision_class() {
    let path = fixture("generic_pi.jsonl");
    let events = parse(&path, Some("pi"), None).unwrap();
    assert!(events
        .iter()
        .all(|e| e.host == "pi" && e.projection == "default"));
    let decision = events
        .iter()
        .find(|e| e.kind == "user_message")
        .expect("user message survives cap");
    assert_eq!(
        decision.classification(),
        Classification::DecisionOrConstraint
    );
}

#[test]
fn generic_fixture_without_session_id_gets_digest_bound_identity() {
    let path = fixture("generic_pi.jsonl");
    let events = parse_source_events(&path, Some("pi")).unwrap();
    let receipt = parse_prefix_receipt(&path, Some("pi")).unwrap().receipt;
    assert!(receipt.session_id.starts_with("derived:pi:"));
    assert_eq!(receipt.session_id.len(), "derived:pi:".len() + 64);
    assert!(events
        .iter()
        .all(|event| event.session_id == receipt.session_id));
}

#[test]
fn parsing_is_deterministic_across_runs() {
    for name in [
        "claude_code.jsonl",
        "codex.jsonl",
        "generic_pi.jsonl",
        "secrets_claude.jsonl",
        "sidechain_claude.jsonl",
    ] {
        let path = fixture(name);
        let a = parse_source_events(&path, None).unwrap();
        let b = parse_source_events(&path, None).unwrap();
        assert_eq!(a, b, "{name} must parse identically across runs");
    }
}

#[test]
fn secrets_are_redacted_and_flagged() {
    let path = fixture("secrets_claude.jsonl");
    let events = parse_source_events(&path, None).unwrap();
    let msg = &events[0];
    assert!(msg.redacted);
    assert!(msg.flags.redacted);
    assert!(msg.text.contains("[REDACTED]"), "{}", msg.text);
    assert!(!msg.text.contains("sk-abcd1234"));
}

#[test]
fn sidechain_rows_are_dropped() {
    let path = fixture("sidechain_claude.jsonl");
    let events = parse_source_events(&path, None).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "visible root message");
}

#[test]
fn prefix_receipt_binds_prefix_and_parser() {
    let path = fixture("claude_code.jsonl");
    let observed = parse_prefix_receipt(&path, None).unwrap();
    assert_eq!(observed.receipt.host, "claude_code");
    assert_eq!(observed.receipt.session_id, "sess-claude-1");
    assert_eq!(observed.receipt.parser_version, PARSER_VERSION);
    assert!(observed.receipt.prefix_digest.starts_with("sha256:"));
    assert_eq!(observed.receipt.parser_digest.len(), 64);
    assert!(observed
        .receipt
        .parser_digest
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));

    // prefixLength equals the file size (every byte belongs to a complete row).
    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(observed.receipt.prefix_length, len);

    // The digest is the sha256 of the whole prefix.
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(&path).unwrap();
    let mut h = Sha256::new();
    h.update(&bytes);
    assert_eq!(
        observed.receipt.prefix_digest,
        format!("sha256:{}", hex::encode(h.finalize()))
    );
}

#[test]
fn detect_host_reads_first_rows_and_falls_back() {
    assert_eq!(
        detect_host(&fixture("claude_code.jsonl")).unwrap(),
        "claude_code"
    );
    assert_eq!(detect_host(&fixture("codex.jsonl")).unwrap(), "codex");
    assert_eq!(detect_host(&fixture("generic_pi.jsonl")).unwrap(), "pi");
}

#[test]
fn missing_file_fails_closed_with_typed_error() {
    let err = parse(&fixture("does_not_exist.jsonl"), None, None).unwrap_err();
    assert!(matches!(err, TranscriptError::Missing { .. }));
}

#[test]
fn unsupported_host_is_rejected() {
    let err = parse(&fixture("claude_code.jsonl"), Some("martian"), None).unwrap_err();
    match err {
        TranscriptError::UnsupportedHost { host } => assert_eq!(host, "martian"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn empty_file_has_no_complete_row() {
    let dir = std::env::temp_dir().join("membrane-transcript-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.jsonl");
    std::fs::write(&path, "").unwrap();
    let err = parse(&path, Some("pi"), None).unwrap_err();
    assert!(matches!(err, TranscriptError::NoCompleteRow { .. }));
    std::fs::remove_file(&path).ok();
}

#[test]
fn malformed_row_fails_closed_with_exact_typed_span() {
    let dir = std::env::temp_dir().join("membrane-transcript-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("malformed.jsonl");
    let first = b"{\"type\":\"adapt_event_v1\",\"host\":\"pi\",\"event\":{\"kind\":\"user_message\",\"text\":\"valid\"}}\n";
    let malformed = b"{not-json}\n";
    let mut body = first.to_vec();
    body.extend_from_slice(malformed);
    std::fs::write(&path, body).unwrap();

    let err = parse_source_events(&path, Some("pi")).unwrap_err();
    match err {
        TranscriptError::MalformedRow {
            row_index,
            byte_start,
            byte_end,
            ..
        } => {
            assert_eq!(row_index, 2);
            assert_eq!(byte_start, first.len() as u64);
            assert_eq!(byte_end, (first.len() + malformed.len()) as u64);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn resolve_session_rejects_substring_matches() {
    let candidates = vec![
        SessionCandidate {
            session_id: "abc-123".into(),
            path: PathBuf::from("/tmp/def-456"),
        },
        SessionCandidate {
            session_id: "other".into(),
            path: PathBuf::from("/tmp/xyz"),
        },
    ];
    // Exact session id wins.
    assert_eq!(
        resolve_session("abc-123", &candidates).unwrap().session_id,
        "abc-123"
    );
    // Substring containment is rejected...
    assert!(resolve_session("bc-12", &candidates).is_none());
    // ...but an exact file-stem fallback is allowed.
    assert_eq!(
        resolve_session("def-456", &candidates).unwrap().session_id,
        "abc-123"
    );
    assert!(resolve_session("", &candidates).is_none());
}

#[test]
fn class_priority_cap_squeezes_only_readonly_tail() {
    // Build a transcript with 10 read-only calls + 1 mutation and confirm the
    // capped projection keeps the mutation and at most the last 6 read-only.
    let dir = std::env::temp_dir().join("membrane-transcript-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.jsonl");
    let mut body = String::new();
    body.push_str(
        r#"{"type":"adapt_event_v1","host":"qwen","event":{"kind":"assistant_message","role":"assistant","text":"start"}}"#,
    );
    body.push('\n');
    body.push_str(
        r#"{"type":"adapt_event_v1","host":"qwen","event":{"kind":"tool_call","role":"assistant","tool":"edit","call_id":"m1","text":"x"}}"#,
    );
    body.push('\n');
    for i in 0..10 {
        let line = format!(
            r#"{{"type":"adapt_event_v1","host":"qwen","event":{{"kind":"tool_result","role":"user","call_id":"r{i}","text":"ok{i}"}}}}"#
        );
        body.push_str(&line);
        body.push('\n');
    }
    std::fs::write(&path, &body).unwrap();

    let capped = parse(&path, Some("qwen"), None).unwrap();
    let readonly = capped
        .iter()
        .filter(|e| e.classification() == Classification::SuccessfulReadonly)
        .count();
    assert!(readonly <= 6, "readonly count {readonly}");
    // Mutation survives the cap.
    assert!(capped
        .iter()
        .any(|e| e.classification() == Classification::Mutation));

    // Uncapped semantic source keeps every event in original order.
    let full = parse_source_events(&path, Some("qwen")).unwrap();
    assert_eq!(full.len(), 12);
    let sequences: Vec<u64> = full.iter().map(|e| e.sequence).collect();
    let mut sorted = sequences.clone();
    sorted.sort_unstable();
    assert_eq!(sequences, sorted);
    std::fs::remove_file(&path).ok();
}

#[test]
fn serialized_events_roundtrip_through_json() {
    let path = fixture("generic_pi.jsonl");
    let events = parse_source_events(&path, Some("pi")).unwrap();
    for ev in &events {
        let json = serde_json::to_string(ev).unwrap();
        let back: membrane_transcript::TranscriptEventV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(*ev, back);
    }
}
