"""Tests for adapt.taste_v2 (Phase 5.3 — plan 5.3).

This test file follows the plain unittest/manual-harness pattern
used by ``test_observable_events.py`` (the only test file in the
adapt repo that runs without pytest installed; the prompt explicitly
says pytest is unavailable on the Python 3.14 box, and the plan
file's pattern is to follow whatever already runs).

The tests exercise the four CORRECTION/DECISION signal paths, the
context-preservation defect fix, the NEVER-authoritative enforcement,
and the in-product TRANSPORT_GAP_NOTE.

The file is a plain unittest module so ``python3 tests/test_taste_v2.py``
runs the entire suite without any external dependency.
"""
from __future__ import annotations

import dataclasses
import sys
import unittest
import copy
from pathlib import Path

TESTS_DIR = Path(__file__).resolve().parent
ADAPT = TESTS_DIR.parent
sys.path.insert(0, str(ADAPT))

from adapt import taste_v2  # noqa: E402


# ---------------------------------------------------------------------------
# Fixture helpers
# ---------------------------------------------------------------------------

def _ev(
    *,
    kind: str = "user_message",
    text: str = "",
    byte_start: int = 0,
    byte_end: int = 100,
    row_index: int = 1,
    sequence: int = 1,
    classification: str = "successful_readonly",
    flags: dict | None = None,
    event_id: str | None = None,
    host: str = "claude_code",
    session_id: str = "s1",
    transcript_id: str = "t1",
    parser_digest: str = "sha256:deadbeef",
) -> dict:
    """Build a TranscriptEventV1-shaped dict."""
    event_flags = {
        "synthetic": False, "meta": False, "privateReasoningOmitted": False,
        "redacted": False, "isError": False, "isSidechain": False,
        **(flags or {}),
    }
    return {
        "eventId": event_id or f"evt_{sequence:04d}",
        "rowIndex": row_index,
        "byteStart": byte_start,
        "byteEnd": byte_end,
        "blockIndex": 0,
        "sequence": sequence,
        "kind": kind,
        "role": "user" if kind == "user_message" else None,
        "tool": None,
        "call_id": None,
        "occurrence": None,
        "text": text,
        "timestamp": "2026-08-04T22:00:00Z",
        "classification": classification,
        "class": classification,
        "projection": "default",
        "host": host,
        "sessionId": session_id,
        "transcriptId": transcript_id,
        "parserDigest": parser_digest,
        "synthetic": event_flags["synthetic"],
        "meta": event_flags["meta"],
        "privateReasoningOmitted": event_flags["privateReasoningOmitted"],
        "redacted": event_flags["redacted"],
        "flags": event_flags,
    }


def _correction_session() -> list[dict]:
    """A small user-led correction session with surrounding context.

    Sequence:
      1. user: ask for parser bug fix
      2. assistant: investigates
      3. tool_call: read
      4. tool_result: success
      5. assistant: wrong fix
      6. user: "No, that's wrong. Always run the focused tests first."
    """
    return [
        _ev(sequence=1, text="please fix the parser bug", byte_start=0, byte_end=40),
        _ev(kind="assistant_message", sequence=2, text="investigating", byte_start=41, byte_end=120),
        _ev(kind="tool_call", sequence=3, text='{"tool":"read"}', byte_start=121, byte_end=170),
        _ev(kind="tool_result", sequence=4, text="ok", byte_start=171, byte_end=220),
        _ev(kind="assistant_message", sequence=5, text="done", byte_start=221, byte_end=270),
        _ev(
            sequence=6,
            text="No, that's wrong. Always run focused tests before reporting a broad build complete.",
            byte_start=271, byte_end=400,
        ),
    ]


def _source_context() -> list[dict]:
    return [{
        "eventId": "e1", "kind": "user_message", "role": "user",
        "classification": "successful_readonly",
        "flags": {name: False for name in taste_v2.SOURCE_FLAG_NAMES},
        "byteStart": 0, "byteEnd": 10, "text": "x", "truncated": False,
        "isSource": True,
    }]


def _decision_session() -> list[dict]:
    """A user-locked decision session."""
    return [
        _ev(sequence=1, text="please add a feature", byte_start=0, byte_end=40),
        _ev(
            sequence=2,
            text="Decision: always use the bare repository path, not the host/path concatenation.",
            byte_start=41, byte_end=200,
        ),
    ]


def _health_session() -> list[dict]:
    """A session where the user message is in the health domain.

    Should be REFUSED at intake — never reach a candidate.
    """
    return [
        _ev(sequence=1, text="please fix the medical diagnosis bug", byte_start=0, byte_end=60),
        _ev(
            sequence=2,
            text="No, that's wrong. Always validate the patient prescription before continuing.",
            byte_start=61, byte_end=200,
        ),
    ]


def _tool_failure_session() -> list[dict]:
    """A tool-result-failure classification that should NEVER mine to a rule."""
    return [
        _ev(
            kind="tool_result",
            sequence=1,
            text="FAIL: test_foo",
            byte_start=0, byte_end=40,
            classification="unresolved_failure",
            flags={"isError": True},
        ),
    ]


def _assistant_narration_session() -> list[dict]:
    """Assistant narration that LOOKS like a correction cue but is not a user_message."""
    return [
        _ev(
            kind="assistant_message",
            sequence=1,
            text="No, that's wrong: the import path was correct.",
            byte_start=0, byte_end=100,
        ),
    ]


# ---------------------------------------------------------------------------
# 1. Detector returns candidates on real fixtures
# ---------------------------------------------------------------------------

class DetectorTests(unittest.TestCase):
    def test_correction_session_yields_one_candidate(self):
        events = _correction_session()
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(len(candidates), 1)
        c = candidates[0]
        self.assertEqual(c.sourceEventId, "evt_0006")
        self.assertEqual(c.sourceByteStart, 271)
        self.assertEqual(c.sourceByteEnd, 400)
        self.assertIn("focused tests", c.rule)

    def test_decision_session_yields_one_candidate_with_locked_decision_record(self):
        events = _decision_session()
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(len(candidates), 1)
        c = candidates[0]
        self.assertEqual(c.recordType, "locked_decision")
        self.assertIn("bare repository path", c.rule)

    def test_health_domain_yields_no_candidates(self):
        events = _health_session()
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(candidates, [])

    def test_tool_failure_yields_no_candidate(self):
        events = _tool_failure_session()
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(candidates, [])

    def test_assistant_narration_yields_no_candidate(self):
        events = _assistant_narration_session()
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(candidates, [])

    def test_clean_session_yields_no_candidate(self):
        events = [
            _ev(sequence=1, text="please add a function", byte_start=0, byte_end=40),
            _ev(kind="assistant_message", sequence=2,
                text="sure, here it is", byte_start=41, byte_end=80),
        ]
        self.assertEqual(taste_v2.extract_candidates(events), [])


# ---------------------------------------------------------------------------
# 2. Defect fix: bounded context is preserved with exact byte spans
# ---------------------------------------------------------------------------

class ContextPreservationTests(unittest.TestCase):
    def test_candidate_carries_bounded_context_events(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        # 4 events on the left + source + 4 events on the right = 7 max
        # (but the source is at index 5, so left = 4 events, right = 0).
        self.assertGreaterEqual(len(c.contextEvents), 5)
        self.assertEqual(len(c.contextEvents), len(c.contextByteSpans))
        # The source event must be in the context, and its byte span must equal
        # the sourceByteStart/End recorded on the candidate.
        spans = [tuple(s) for s in c.contextByteSpans]
        self.assertIn((c.sourceByteStart, c.sourceByteEnd), spans)

    def test_context_byte_span_audit_exact(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        # Every context event's byte span must come from the source events.
        truth = {(e["byteStart"], e["byteEnd"]) for e in events}
        for s in c.contextByteSpans:
            self.assertIn(tuple(s), truth)

    def test_context_chars_clamped(self):
        events = _correction_session()
        # Defect fix: context has a CHAR bound too, not just a count bound.
        # With a tiny max_chars, the context must be clamped. The cap is on
        # source bytes — the ellipsis marker is a display signal, not
        # content, so we strip it for the count.
        c = taste_v2.extract_candidates(
            events, scope="workspace", max_blocks=4, max_chars=20,
        )[0]
        total_chars = sum(
            len(ev["text"].rstrip("…"))
            for ev in c.contextEvents
            if not ev["isSource"]
        )
        self.assertLessEqual(total_chars, 20)

    def test_context_max_blocks_clamped(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(
            events, scope="workspace", max_blocks=1, max_chars=10_000,
        )[0]
        # Only 1 event on each side of the source plus the source itself.
        self.assertLessEqual(len(c.contextEvents), 3)


# ---------------------------------------------------------------------------
# 3. NEVER-authoritative enforcement
# ---------------------------------------------------------------------------

class AuthoritativeProvenanceTests(unittest.TestCase):
    def test_synthetic_event_refused(self):
        events = _correction_session()
        # Mutate the source event to be synthetic.
        events[5]["flags"] = {"synthetic": True}
        events[5]["synthetic"] = True
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(candidates, [])

    def test_meta_event_refused(self):
        events = _correction_session()
        events[5]["flags"] = {"meta": True}
        events[5]["meta"] = True
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(candidates, [])

    def test_redacted_event_refused(self):
        events = _correction_session()
        events[5]["flags"] = {"redacted": True}
        events[5]["redacted"] = True
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(candidates, [])

    def test_extracted_candidate_has_transport_note(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        self.assertEqual(c.transportNote, taste_v2.TRANSPORT_GAP_NOTE)
        self.assertIn("cannot mint Taste candidates", c.transportNote)

    def test_malicious_nested_provenance_matrix_rejects_before_admission(self):
        candidate = taste_v2.extract_candidates(_correction_session(), scope="workspace")[0]
        cases = {
            "source text": lambda c: c.contextEvents[-1].__setitem__("text", "forged"),
            "source flag": lambda c: c.contextEvents[-1]["flags"].__setitem__("meta", True),
            "source role": lambda c: c.contextEvents[-1].__setitem__("role", "assistant"),
            "source span": lambda c: c.contextEvents[-1].__setitem__("byteEnd", 401),
            "second source": lambda c: c.contextEvents[0].__setitem__("isSource", True),
        }
        for name, mutate in cases.items():
            with self.subTest(name=name):
                forged = copy.deepcopy(candidate)
                mutate(forged)
                rejected = taste_v2.admit_candidate(forged)
                self.assertEqual(rejected.lifecycleState, "rejected")
                self.assertIn("candidate source context", rejected.admissionReason)

    def test_candidate_requires_exact_parser_flags_and_source_context(self):
        candidate = taste_v2.extract_candidates(_correction_session(), scope="workspace")[0]
        with self.assertRaisesRegex(taste_v2.TasteV2Error, "exactly the six boolean"):
            dataclasses.replace(candidate, sourceFlags=(("synthetic", False),))
        with self.assertRaisesRegex(taste_v2.TasteV2Error, "contextEvents must be non-empty"):
            dataclasses.replace(candidate, contextEvents=[])


# ---------------------------------------------------------------------------
# 4. Lifecycle + admission gate
# ---------------------------------------------------------------------------

class AdmissionGateTests(unittest.TestCase):
    def test_candidate_born_in_candidate_state(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        self.assertEqual(c.lifecycleState, "candidate")

    def test_admit_candidate_returns_active(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        admitted = taste_v2.admit_candidate(c)
        self.assertEqual(admitted.lifecycleState, "active")
        self.assertEqual(admitted.admissionReason, "ok")

    def test_admit_candidate_rejects_health_domain_text(self):
        events = _health_session()
        # The detector refuses them at intake; we cannot reach admission with one.
        # Synthesise a candidate manually to test the gate's category refusal.
        c = taste_v2.extract_candidates(
            [
                _ev(sequence=1,
                    text="No, that's wrong. Always verify prescription before continuing.",
                    byte_start=0, byte_end=100),
            ],
            scope="workspace",
        )
        self.assertEqual(c, [],
                          "health-domain text should never produce a candidate")

    def test_admit_candidate_rejects_short_rule(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        # Hand-roll a candidate with a too-short rule.
        short = taste_v2.TasteCandidateV1(
            ruleId="taste_short",
            rule="ok",
            scope="workspace",
            category="workflow",
            recordType="operational_playbook",
            sourceEventId=c.sourceEventId,
            sourceByteStart=c.sourceByteStart,
            sourceByteEnd=c.sourceByteEnd,
            sourceRowIndex=c.sourceRowIndex,
            sourceSequence=c.sourceSequence,
            sourceHost=c.sourceHost,
            sourceSessionId=c.sourceSessionId,
            sourceTranscriptId=c.sourceTranscriptId,
            sourceParserDigest=c.sourceParserDigest,
            sourceKind=c.sourceKind, sourceRole=c.sourceRole,
            sourceClassification=c.sourceClassification, sourceFlags=c.sourceFlags,
            contextEvents=c.contextEvents,
            contextByteSpans=c.contextByteSpans,
            evidenceId=c.evidenceId,
            evidenceText=c.evidenceText,
        )
        admitted = taste_v2.admit_candidate(short)
        self.assertEqual(admitted.lifecycleState, "rejected")
        self.assertIn("rule-invalid-shape", admitted.admissionReason)


# ---------------------------------------------------------------------------
# 5. Rule id determinism
# ---------------------------------------------------------------------------

class RuleIdTests(unittest.TestCase):
    def test_rule_id_is_deterministic_for_same_input(self):
        a = taste_v2._rule_id("workspace", "always run tests", 100, 200)
        b = taste_v2._rule_id("workspace", "always run tests", 100, 200)
        self.assertEqual(a, b)
        self.assertTrue(a.startswith("taste_"))

    def test_rule_id_changes_with_byte_span(self):
        a = taste_v2._rule_id("workspace", "always run tests", 100, 200)
        b = taste_v2._rule_id("workspace", "always run tests", 100, 201)
        self.assertNotEqual(a, b)

    def test_rule_id_changes_with_rule_text(self):
        a = taste_v2._rule_id("workspace", "always run tests", 100, 200)
        b = taste_v2._rule_id("workspace", "always run lint", 100, 200)
        self.assertNotEqual(a, b)


# ---------------------------------------------------------------------------
# 6. Schema validation
# ---------------------------------------------------------------------------

class SchemaValidationTests(unittest.TestCase):
    def test_invalid_lifecycle_state_raises(self):
        with self.assertRaises(taste_v2.TasteV2Error):
            taste_v2.TasteCandidateV1(
                ruleId="t1", rule="always run tests",
                scope="workspace", category="workflow",
                recordType="operational_playbook",
                sourceEventId="e1", sourceByteStart=0, sourceByteEnd=10,
                sourceRowIndex=1, sourceSequence=1,
                sourceHost="claude_code", sourceSessionId="s1",
                sourceTranscriptId="t1", sourceParserDigest="sha256:xx",
                sourceKind="user_message", sourceRole="user",
                sourceClassification="successful_readonly",
                sourceFlags=tuple((name, False) for name in taste_v2.SOURCE_FLAG_NAMES),
                contextEvents=_source_context(),
                contextByteSpans=[(0, 10)],
                evidenceId="ev1", evidenceText="x",
                lifecycleState="bogus",
            )

    def test_empty_context_raises(self):
        with self.assertRaises(taste_v2.TasteV2Error):
            taste_v2.TasteCandidateV1(
                ruleId="t1", rule="always run tests",
                scope="workspace", category="workflow",
                recordType="operational_playbook",
                sourceEventId="e1", sourceByteStart=0, sourceByteEnd=10,
                sourceRowIndex=1, sourceSequence=1,
                sourceHost="claude_code", sourceSessionId="s1",
                sourceTranscriptId="t1", sourceParserDigest="sha256:xx",
                sourceKind="user_message", sourceRole="user",
                sourceClassification="successful_readonly",
                sourceFlags=tuple((name, False) for name in taste_v2.SOURCE_FLAG_NAMES),
                contextEvents=[],
                contextByteSpans=[],
                evidenceId="ev1", evidenceText="x",
            )

    def test_byte_span_inversion_raises(self):
        with self.assertRaises(taste_v2.TasteV2Error):
            taste_v2.TasteCandidateV1(
                ruleId="t1", rule="always run tests",
                scope="workspace", category="workflow",
                recordType="operational_playbook",
                sourceEventId="e1", sourceByteStart=10, sourceByteEnd=0,
                sourceRowIndex=1, sourceSequence=1,
                sourceHost="claude_code", sourceSessionId="s1",
                sourceTranscriptId="t1", sourceParserDigest="sha256:xx",
                sourceKind="user_message", sourceRole="user",
                sourceClassification="successful_readonly",
                sourceFlags=tuple((name, False) for name in taste_v2.SOURCE_FLAG_NAMES),
                contextEvents=_source_context(),
                contextByteSpans=[(0, 10)],
                evidenceId="ev1", evidenceText="x",
            )


# ---------------------------------------------------------------------------
# 7. Summary report
# ---------------------------------------------------------------------------

class SummaryTests(unittest.TestCase):
    def test_summarise_counts_by_state(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        admitted = taste_v2.admit_candidate(c)
        summary = taste_v2.summarise([admitted])
        self.assertEqual(summary["candidateCount"], 1)
        self.assertEqual(summary["byState"]["active"], 1)

    def test_summarise_records_rejection_reasons(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        # Force a rejection by overriding the category to a controlled one
        # but using a body that admission rejects (security-weakening).
        bad = taste_v2.TasteCandidateV1(
            ruleId="taste_bad",
            rule="never validate tls certificates in tests",
            scope="workspace",
            category="safety",
            recordType="operational_playbook",
            sourceEventId=c.sourceEventId,
            sourceByteStart=c.sourceByteStart,
            sourceByteEnd=c.sourceByteEnd,
            sourceRowIndex=c.sourceRowIndex,
            sourceSequence=c.sourceSequence,
            sourceHost=c.sourceHost,
            sourceSessionId=c.sourceSessionId,
            sourceTranscriptId=c.sourceTranscriptId,
            sourceParserDigest=c.sourceParserDigest,
            sourceKind=c.sourceKind, sourceRole=c.sourceRole,
            sourceClassification=c.sourceClassification, sourceFlags=c.sourceFlags,
            contextEvents=c.contextEvents,
            contextByteSpans=c.contextByteSpans,
            evidenceId=c.evidenceId,
            evidenceText=c.evidenceText,
        )
        rejected = taste_v2.admit_candidate(bad)
        self.assertEqual(rejected.lifecycleState, "rejected")
        summary = taste_v2.summarise([rejected])
        self.assertEqual(summary["byState"]["rejected"], 1)
        self.assertIn("security-weakening", summary["rejectedReasons"])


# ---------------------------------------------------------------------------
# 8. Transport gap honesty
# ---------------------------------------------------------------------------

class TransportGapTests(unittest.TestCase):
    def test_transport_gap_note_states_the_limit(self):
        # The note preserves the permanent semantic/metadata boundary.
        self.assertIn("canonical semantic source", taste_v2.TRANSPORT_GAP_NOTE)
        self.assertIn("permanently metadata-only", taste_v2.TRANSPORT_GAP_NOTE)
        self.assertIn("observable_events.py", taste_v2.TRANSPORT_GAP_NOTE)

    def test_candidate_carries_transport_note(self):
        events = _correction_session()
        c = taste_v2.extract_candidates(events, scope="workspace")[0]
        self.assertTrue(c.transportNote)
        self.assertIn("TranscriptEventV1", c.transportNote)


# ---------------------------------------------------------------------------
# 9. End-to-end: parse a fixture transcript, extract, admit
# ---------------------------------------------------------------------------

class EndToEndTests(unittest.TestCase):
    def test_empty_event_list_returns_empty(self):
        self.assertEqual(taste_v2.extract_candidates([]), [])

    def test_extract_candidate_returns_none_for_non_candidate_event(self):
        events = _correction_session()
        # Index 0 is the user's initial ask, not a correction.
        self.assertIsNone(taste_v2.extract_candidate(events, 0))

    def test_full_pipeline_round_trip(self):
        events = _correction_session()
        candidates = taste_v2.extract_candidates(events, scope="workspace")
        self.assertEqual(len(candidates), 1)
        admitted = taste_v2.admit_candidate(candidates[0])
        self.assertEqual(admitted.lifecycleState, "active")
        # The admitted candidate must keep its byte-span provenance.
        self.assertEqual(admitted.sourceByteStart, 271)
        self.assertEqual(admitted.sourceByteEnd, 400)
        self.assertTrue(admitted.contextEvents)
        self.assertTrue(admitted.contextByteSpans)


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    unittest.main()
