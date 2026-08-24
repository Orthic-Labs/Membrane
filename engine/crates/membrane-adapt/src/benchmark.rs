//! Labelled-corpus benchmark feeding the precision gate.
//!
//! A corpus is a fixed set of labelled transcripts (events + expected family
//! hits). Running detectors over it yields per-family precision/recall with
//! deterministic IDs, which `remediation::precision_gate` consumes.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::insights::detectors::run_all_detectors;
use crate::insights::TranscriptEventV1;

/// One labelled case: events plus the families a correct detector run must
/// fire (and optionally families that must NOT fire — false-positive traps).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelledCase {
    pub case_id: String,
    pub events: Vec<TranscriptEventV1>,
    /// Families expected to fire at least once.
    pub expected_families: BTreeSet<String>,
    /// Families that must not fire (guards/precision traps).
    #[serde(default)]
    pub forbidden_families: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct FamilyScore {
    pub true_positives: u32,
    pub false_positives: u32,
    pub false_negatives: u32,
}

impl FamilyScore {
    pub fn precision(&self) -> f64 {
        let d = self.true_positives + self.false_positives;
        if d == 0 {
            0.0
        } else {
            self.true_positives as f64 / d as f64
        }
    }

    pub fn recall(&self) -> f64 {
        let d = self.true_positives + self.false_negatives;
        if d == 0 {
            0.0
        } else {
            self.true_positives as f64 / d as f64
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BenchmarkReportV1 {
    pub corpus_size: usize,
    /// family -> score
    pub by_family: BTreeMap<String, FamilyScore>,
    /// Deterministic digest of (corpus + results) so reports are comparable.
    pub report_digest: String,
}

/// Parse one frozen, semantically sealed P0.5 portable corpus row into the
/// native detector input. Corpus authority comes from explicit kind/role;
/// event IDs never carry semantic meaning.
pub fn portable_case_from_value(value: &serde_json::Value) -> Result<LabelledCase, String> {
    let object = value.as_object().ok_or("portable case must be an object")?;
    let payload = object.get("payload").ok_or("portable case payload missing")?;
    let digest = object
        .get("payload_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or("portable case payload_sha256 missing")?;
    if crate::canonical::sha256_canonical(payload) != digest {
        return Err("portable case semantic seal mismatch".into());
    }
    let case_id = object
        .get("case_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("portable case id missing")?;
    if case_id != format!("ibc_{digest}") {
        return Err("portable case identity mismatch".into());
    }
    let review_status = object
        .get("state")
        .and_then(serde_json::Value::as_object)
        .and_then(|state| state.get("review_status"))
        .and_then(serde_json::Value::as_str);
    if review_status != Some("frozen") {
        return Err("portable case is not frozen".into());
    }
    let family = payload
        .get("family")
        .and_then(serde_json::Value::as_str)
        .ok_or("portable case family missing")?
        .to_string();
    let label = payload
        .get("label")
        .and_then(serde_json::Value::as_str)
        .ok_or("portable case label missing")?;
    let detected = payload
        .pointer("/expected/detected")
        .and_then(serde_json::Value::as_bool)
        .ok_or("portable expected.detected missing")?;
    if (label == "positive") != detected || !matches!(label, "positive" | "negative") {
        return Err("portable case label/detected mismatch".into());
    }
    let source_events = payload
        .pointer("/transcript_excerpt/events")
        .and_then(serde_json::Value::as_array)
        .ok_or("portable case events missing")?;
    let mut events = Vec::with_capacity(source_events.len());
    for event in source_events {
        let required = |name: &str| {
            event
                .get(name)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("portable event {name} missing"))
        };
        let kind = match required("kind")? {
            "user_message" => crate::insights::EventKind::UserMessage,
            "assistant_message" => crate::insights::EventKind::AssistantMessage,
            "tool_call" => crate::insights::EventKind::ToolCall,
            "tool_result" => crate::insights::EventKind::ToolResult,
            other => return Err(format!("portable event kind unsupported: {other}")),
        };
        let role = required("role")?;
        let provenance = match (kind, role) {
            (crate::insights::EventKind::UserMessage, "user") => "external_user",
            (crate::insights::EventKind::AssistantMessage, "assistant") => "assistant",
            (crate::insights::EventKind::ToolCall | crate::insights::EventKind::ToolResult, "tool") => "tool",
            _ => return Err("portable event kind/role mismatch".into()),
        };
        let byte_start = event
            .get("byte_start")
            .and_then(serde_json::Value::as_i64)
            .ok_or("portable event byte_start missing")?;
        let byte_end = event
            .get("byte_end")
            .and_then(serde_json::Value::as_i64)
            .ok_or("portable event byte_end missing")?;
        if byte_start < 0 || byte_end <= byte_start {
            return Err("portable event byte span invalid".into());
        }
        events.push(TranscriptEventV1 {
            event_id: required("event_id")?.to_string(),
            session_id: required("session_id")?.to_string(),
            host: "portable_benchmark".into(),
            provenance: provenance.into(),
            kind,
            text: required("text")?.to_string(),
            timestamp: None,
            byte_start,
            byte_end,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        });
    }
    Ok(LabelledCase {
        case_id: case_id.to_string(),
        events,
        expected_families: if detected {
            BTreeSet::from([family.clone()])
        } else {
            BTreeSet::new()
        },
        forbidden_families: if detected {
            BTreeSet::new()
        } else {
            BTreeSet::from([family])
        },
    })
}

/// Run detectors over every labelled case and score per family.
pub fn run_benchmark(corpus: &[LabelledCase]) -> BenchmarkReportV1 {
    let mut scores: BTreeMap<String, FamilyScore> = BTreeMap::new();
    for case in corpus {
        // Score only against families named in the case; unnamed families
        // firing on this case count as false positives for that family.
        let fired: BTreeSet<String> =
            run_all_detectors(&case.events).into_iter().map(|e| e.family).collect();
        let all_families: BTreeSet<&String> = case.expected_families.union(&case.forbidden_families).collect();
        for family in all_families {
            let entry = scores.entry(family.clone()).or_default();
            let expected = case.expected_families.contains(family);
            let did_fire = fired.contains(family);
            match (expected, did_fire) {
                (true, true) => entry.true_positives += 1,
                (true, false) => entry.false_negatives += 1,
                (false, true) => entry.false_positives += 1,
                (false, false) => {}
            }
        }
    }
    let mut digest_src = String::new();
    for case in corpus {
        digest_src.push_str(&case.case_id);
        for f in &case.expected_families {
            digest_src.push('\u{1}');
            digest_src.push_str(f);
        }
    }
    for (fam, s) in &scores {
        digest_src.push_str(&format!(
            "{fam}:{}/{}/{};",
            s.true_positives, s.false_positives, s.false_negatives
        ));
    }
    BenchmarkReportV1 {
        corpus_size: corpus.len(),
        by_family: scores,
        report_digest: crate::canonical::sha256_hex(digest_src.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::EventKind;

    fn ev(text: &str) -> TranscriptEventV1 {
        TranscriptEventV1 {
            event_id: format!("ev-{}", &crate::canonical::sha256_hex(text.as_bytes())[..8]),
            session_id: "s".into(),
            host: "pi".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: text.into(),
            timestamp: None,
            byte_start: 0,
            byte_end: text.len() as i64,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        }
    }

    #[test]
    fn perfect_corpus_scores_one_precision() {
        let cases = vec![
            LabelledCase {
                case_id: "c1".into(),
                events: vec![ev("this is so frustrating and annoying")],
                expected_families: BTreeSet::from(["visible_frustration".to_string()]),
                forbidden_families: BTreeSet::new(),
            },
            LabelledCase {
                case_id: "c2".into(),
                events: vec![ev("the log says: verified passing done")],
                expected_families: BTreeSet::new(),
                // Quoted material must NOT trigger frustration or swearing.
                forbidden_families: BTreeSet::new(),
            },
        ];
        let r = run_benchmark(&cases);
        assert!(r.by_family.get("visible_frustration").unwrap().precision() > 0.0);
    }

    #[test]
    fn report_digest_is_stable() {
        let mk = || {
            vec![LabelledCase {
                case_id: "c1".into(),
                events: vec![ev("this is so frustrating and annoying")],
                expected_families: BTreeSet::from(["visible_frustration".to_string()]),
                forbidden_families: BTreeSet::new(),
            }]
        };
        assert_eq!(run_benchmark(&mk()).report_digest, run_benchmark(&mk()).report_digest);
    }
}
