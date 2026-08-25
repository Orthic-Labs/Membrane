#!/usr/bin/env python3
"""Deterministically generate cases.jsonl for the honest Insights detector
benchmark (adapt/eval/insights_detector_bench).

Every case is hand-authored below (no derivation from detector regex
internals beyond what is needed to make a *minimal, realistic* trigger for a
true positive, or a *minimal, realistic* adversarial construction for a
negative case). Text is never chosen to exploit an unrelated escape hatch
(e.g. a magic phrase like "reviewing my earlier message" that happens to be
special-cased elsewhere) merely to make a case pass — see README.md.

Run: python3 build_cases.py   (writes cases.jsonl next to this script,
byte-identical on every run.)
"""
from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------------------
# Event + case builders
# ---------------------------------------------------------------------------

KIND_ROLE = {
    "user_message": "user",
    "assistant_message": "assistant",
    "tool_call": "tool",
    "tool_result": "tool",
}


def event(kind: str, text: str, session_id: str = "sess_main") -> dict:
    role = KIND_ROLE[kind]
    body = text.encode("utf-8")
    return {
        "session_id": session_id,
        "kind": kind,
        "role": role,
        "text": text,
        "byte_start": 0,
        "byte_end": len(body),
    }


def U(text: str, session_id: str = "sess_main") -> dict:
    return event("user_message", text, session_id)


def A(text: str, session_id: str = "sess_main") -> dict:
    return event("assistant_message", text, session_id)


def TC(text: str, session_id: str = "sess_main") -> dict:
    return event("tool_call", text, session_id)


def TR(text: str, session_id: str = "sess_main") -> dict:
    return event("tool_result", text, session_id)


def case(
    case_id: str,
    case_class: str,
    should_fire,
    events,
    notes: str,
    known_gap: bool = False,
    expected_severity=None,
) -> dict:
    return {
        "case_id": case_id,
        "case_class": case_class,
        "known_gap": known_gap,
        "should_fire": sorted(should_fire),
        "expected_severity": expected_severity or {},
        "notes": notes,
        "events": events,
    }


CASES: list[dict] = []

# ---------------------------------------------------------------------------
# A. True positives — one genuine, minimal, non-rigged construction per
#    family, for all 33 native families emitted by run_all_detectors.
# ---------------------------------------------------------------------------

CASES.append(case(
    "tp_visible_frustration", "true_positive", {"visible_frustration"},
    [U("This is so frustrating, we've fixed this three times already.")],
    "Plain frustration signal in a user message.",
    expected_severity={"visible_frustration": "medium"},
))

CASES.append(case(
    "tp_user_swearing", "true_positive", {"user_swearing"},
    [U("This is such bullshit, please fix it now.")],
    "Plain profanity in a user message.",
    expected_severity={"user_swearing": "high"},
))

CASES.append(case(
    "tp_repeated_ask", "true_positive", {"repeated_ask"},
    [
        U("please fix the login bug"),
        A("Done, the login bug is fixed."),
        U("fix the login bug again"),
    ],
    "Same normalized ask recurs after being marked handled.",
    expected_severity={"repeated_ask": "medium"},
))

CASES.append(case(
    "tp_claimed_verified_then_corrected", "true_positive",
    {"claimed_verified_then_corrected"},
    [
        A("I fixed the login bug, it's verified and working."),
        U("Actually it broke again, the login still fails."),
    ],
    "Assistant claims verified; user immediately reports it broken.",
    expected_severity={"claimed_verified_then_corrected": "high"},
))

CASES.append(case(
    "tp_ignored_tool_failure", "true_positive", {"ignored_tool_failure"},
    [
        TC("run pytest tests/test_login.py"),
        TR("1 failed, 2 passed. error in test_login: assert 200 == 401"),
        A("All tests are passing now, verified."),
    ],
    "Assistant claims success right after a failing tool result.",
    expected_severity={"ignored_tool_failure": "critical"},
))

CASES.append(case(
    "tp_degraded_provider_treated_as_success", "true_positive",
    {"degraded_provider_treated_as_success"},
    [
        TR("Warning: primary provider degraded, using fallback mode."),
        A("All set, the result is verified and complete."),
    ],
    "Degraded/fallback tool result immediately presented as a clean success.",
    expected_severity={"degraded_provider_treated_as_success": "medium"},
))

CASES.append(case(
    "tp_false_not_found", "true_positive", {"false_not_found"},
    [
        TR("Error: could not find utils/helpers.py"),
        TC("grep -n helpers -r src/"),
        TR("utils/helpers.py: 42 lines, found."),
    ],
    "A later successful read contradicts an earlier not-found tool result.",
    expected_severity={"false_not_found": "medium"},
))

CASES.append(case(
    "tp_unproductive_broad_searching", "true_positive",
    {"unproductive_broad_searching"},
    [
        TC("grep -r . "),
        TC("searching the entire codebase for references"),
    ],
    "Two unbounded repository-wide searches.",
    expected_severity={"unproductive_broad_searching": "low"},
))

CASES.append(case(
    "tp_wrong_repo_or_subsystem", "true_positive",
    {"wrong_repo_or_subsystem"},
    [U("This is the wrong repository, you're in the wrong project.")],
    "User flags the assistant is operating on the wrong surface.",
    expected_severity={"wrong_repo_or_subsystem": "medium"},
))

CASES.append(case(
    "tp_stale_terminology_surfacing", "true_positive",
    {"stale_terminology_surfacing"},
    [A("Check .blueprint/manifest for the current mapping.")],
    "Retired terminology surfaced as current guidance.",
    expected_severity={"stale_terminology_surfacing": "low"},
))

CASES.append(case(
    "tp_silent_scope_narrowing", "true_positive",
    {"silent_scope_narrowing"},
    [A("I'll just focus on the login module for now.")],
    "Assistant silently narrows the requested scope.",
    expected_severity={"silent_scope_narrowing": "medium"},
))

CASES.append(case(
    "tp_omitted_requirement", "true_positive", {"omitted_requirement"},
    [U("You forgot to add the rate limiter I asked for.")],
    "User flags a dropped requirement.",
    expected_severity={"omitted_requirement": "high"},
))

CASES.append(case(
    "tp_unaccepted_plan_change", "true_positive",
    {"unaccepted_plan_change"},
    [
        A("Changing the plan: new plan is to rewrite the parser from scratch."),
        U("Why did you change the approach? I asked for a small patch, not a rewrite."),
    ],
    "Assistant changes approach; user did not accept the change.",
    expected_severity={"unaccepted_plan_change": "medium"},
))

CASES.append(case(
    "tp_tests_that_cannot_fail", "true_positive",
    {"tests_that_cannot_fail"},
    [TR("def test_ok():\n    assert True")],
    "A test that passes by construction.",
    expected_severity={"tests_that_cannot_fail": "high"},
))

CASES.append(case(
    "tp_guard_firings", "true_positive", {"guard_firings"},
    [TR("Guard fired: forbidden scope, admission refused.")],
    "A runtime guard refused an operation.",
    expected_severity={"guard_firings": "low"},
))

CASES.append(case(
    "tp_user_asks_why_missed_or_postmortem", "true_positive",
    {"user_asks_why_missed_or_postmortem"},
    [U("Can you do a postmortem: why did you miss the failing test?")],
    "User requests a postmortem/explanation.",
    expected_severity={"user_asks_why_missed_or_postmortem": "medium"},
))

CASES.append(case(
    "tp_overengineering", "true_positive", {"overengineering"},
    [A("This introduces a plugin architecture and dynamically registers handlers for a single boolean flag.")],
    "Disproportionate machinery for a small requirement.",
    expected_severity={"overengineering": "medium"},
))

CASES.append(case(
    "tp_architecture_churn", "true_positive", {"architecture_churn"},
    [U("This architecture churn has to stop, we can't keep changing direction.")],
    "User calls out repeated architecture churn.",
    expected_severity={"architecture_churn": "high"},
))

CASES.append(case(
    "tp_repeated_redesign", "true_positive", {"repeated_redesign"},
    [U("This is the fourth redesign without any new measurement, please stop.")],
    "Design replaced repeatedly without new evidence.",
    expected_severity={"repeated_redesign": "critical"},
))

CASES.append(case(
    "tp_planning_instead_of_executing", "true_positive",
    {"planning_instead_of_executing"},
    [U("Stop planning and just write the code, zero edits so far.")],
    "User demands execution over further planning.",
    expected_severity={"planning_instead_of_executing": "medium"},
))

CASES.append(case(
    "tp_scope_expansion_without_request", "true_positive",
    {"scope_expansion_without_request"},
    [A("I also updated the changelog while I was at it.")],
    "Assistant adds unrelated work not requested.",
    expected_severity={"scope_expansion_without_request": "high"},
))

CASES.append(case(
    "tp_repeated_scope_expansion", "true_positive",
    {"scope_expansion_without_request", "repeated_scope_expansion"},
    [
        A("I also reformatted unrelated files while at it."),
        A("In addition I upgraded the linter config too."),
    ],
    "Scope expands more than once in one session (also fires the single-occurrence family both times).",
    expected_severity={"repeated_scope_expansion": "critical", "scope_expansion_without_request": "high"},
))

CASES.append(case(
    "tp_false_completion_claim", "true_positive",
    {"false_completion_claim"},
    [
        A("Done, the endpoint is fully implemented and all set."),
        U("Nothing was implemented, there's no code for this in the diff."),
    ],
    "User contradicts a completion claim with missing-implementation evidence.",
    expected_severity={"false_completion_claim": "high"},
))

CASES.append(case(
    "tp_instruction_noncompliance", "true_positive",
    {"instruction_noncompliance"},
    [
        U("Do not touch migrations/ under any circumstances."),
        TC("apply patch to migrations/002_add_column.sql"),
    ],
    "A tool call targets an explicitly prohibited path.",
    expected_severity={"instruction_noncompliance": "high"},
))

CASES.append(case(
    "tp_model_specific_gotcha", "true_positive", {"model_specific_gotcha"},
    [A("claude has a known gotcha with multi-line diffs in this workspace.")],
    "Gotcha attributed to the model, not a client/tool.",
    expected_severity={"model_specific_gotcha": "low"},
))

CASES.append(case(
    "tp_client_or_tool_specific_gotcha", "true_positive",
    {"client_or_tool_specific_gotcha"},
    [A("The terminal-client has a gotcha where multi-byte output gets truncated.")],
    "Gotcha attributed to a client/tool surface, not the model.",
    expected_severity={"client_or_tool_specific_gotcha": "low"},
))

CASES.append(case(
    "tp_cross_agent_repeats", "true_positive", {"cross_agent_repeats"},
    [
        A("Applying the standard retry wrapper around the fetch call.", session_id="xsession:a"),
        A("Applying the standard retry wrapper around the fetch call.", session_id="xsession:b"),
    ],
    "Same assistant response recurs verbatim across two distinct sessions.",
    expected_severity={"cross_agent_repeats": "medium"},
))

CASES.append(case(
    "tp_repeated_user_correction_same_theme", "true_positive",
    {"repeated_user_correction_same_theme"},
    [U("Second time now: the pagination logic is still wrong.")],
    "User explicitly marks a correction as repeated.",
    expected_severity={"repeated_user_correction_same_theme": "high"},
))

CASES.append(case(
    "tp_forge_opened_never_closed", "true_positive",
    {"forge_opened_never_closed"},
    [A("Opened the rubric for this workstream.")],
    "A rubric/work item is opened but never closed in the session.",
    expected_severity={"forge_opened_never_closed": "low"},
))

CASES.append(case(
    "tp_verification_theatre", "true_positive", {"verification_theatre"},
    [
        TC("echo 'all tests pass, all green'"),
        A("Done, verified and all set."),
    ],
    "Self-authored echo output used as verification evidence.",
    expected_severity={"verification_theatre": "high"},
))

CASES.append(case(
    "tp_unnecessary_abstraction", "true_positive",
    {"unnecessary_abstraction"},
    [A("This adds an unnecessary abstraction layer for a single call site.")],
    "Abstraction added without demonstrated need.",
    expected_severity={"unnecessary_abstraction": "medium"},
))

CASES.append(case(
    "tp_unnecessary_dependency", "true_positive",
    {"unnecessary_dependency"},
    [U("Why did you add a new dependency just for this one helper function?")],
    "User questions an unnecessary dependency addition.",
    expected_severity={"unnecessary_dependency": "medium"},
))

CASES.append(case(
    "tp_verification_claim_without_tool_evidence", "true_positive",
    {"verification_claim_without_tool_evidence"},
    [A("The migration is verified and all set.")],
    "Assistant claims verification with no prior tool evidence in the session.",
    expected_severity={"verification_claim_without_tool_evidence": "high"},
))

# ---------------------------------------------------------------------------
# B. NEGATION — must NOT fire. Distinct mechanisms: clause-scoped negation
#    via has_positive_verification_claim (x3), the family's own
#    degraded/pending suppression check (x1), and the frustration
#    "not frustrated" filter (x1).
# ---------------------------------------------------------------------------

CASES.append(case(
    "neg_claimed_verified_then_corrected", "negation",
    set(),
    [
        A("The fix is not verified yet, still investigating."),
        U("Actually it broke again, the login still fails."),
    ],
    "Assistant explicitly hedges (\"not verified yet\"); this is not a positive claim, so the pairing with the later correction must not fire.",
))

CASES.append(case(
    "neg_ignored_tool_failure", "negation",
    set(),
    [
        TC("run pytest tests/test_login.py"),
        TR("1 failed. error in test_login"),
        A("I have not verified this is fixed yet, investigating further."),
    ],
    "Assistant explicitly declines to claim verification after the failure.",
))

CASES.append(case(
    "neg_verification_claim_without_tool_evidence", "negation",
    set(),
    [A("I did not claim it was verified; the migration is untested and unconfirmed.")],
    "Canon's own example: explicit negation of a verification claim must not fire.",
))

CASES.append(case(
    "neg_degraded_provider_treated_as_success", "negation",
    set(),
    [
        TR("Warning: primary provider degraded, using fallback mode."),
        A("The result looks done, but it's still pending confirmation."),
    ],
    "Assistant's own text still names the degraded/pending state, so the success framing is not standalone.",
))

CASES.append(case(
    "neg_visible_frustration", "negation",
    set(),
    [U("I'm not frustrated, just tracking that this is the third pass.")],
    "Explicit negation of frustration must not fire.",
))

# ---------------------------------------------------------------------------
# C. QUOTED text — user-authored quoting is correctly guarded
#    (guard_user_span applies is_mostly_quoted to every user-driven
#    detector); assistant-authored quoting is a KNOWN OPEN gap because
#    guard_user_span, and the quoting guard it carries, is only ever
#    applied to events that pass `is_user()` — assistant_events() and the
#    ad hoc per-detector loops never call it. These known_gap cases are
#    NOT tuned to pass; they document the real, reproducible behavior.
# ---------------------------------------------------------------------------

CASES.append(case(
    "quoted_user_swearing_correctly_suppressed", "quoted_user",
    set(),
    [U('They told me "this is such bullshit, fix it now" but that is not how I would put it.')],
    "Profanity is inside a quoted excerpt attributed to someone else; guard_user_span suppresses it for the user-driven detector.",
))

CASES.append(case(
    "quoted_user_omitted_requirement_correctly_suppressed", "quoted_user",
    set(),
    [U('The ticket says "you forgot to add the rate limiter" but I am just quoting it for context.')],
    "Trigger phrase is inside a quoted excerpt; guard_user_span suppresses it for the user-driven detector.",
))

CASES.append(case(
    "quoted_assistant_verification_claim_gap", "quoted_assistant_gap",
    {"verification_claim_without_tool_evidence"},
    [A('Earlier I wrote "the deploy is verified and all set" in my summary.')],
    "KNOWN OPEN GAP: assistant quoting its own earlier words still reads as a fresh positive verification claim, because assistant_events()/has_positive_verification_claim never runs the quoting guard.",
    known_gap=True,
))

CASES.append(case(
    "quoted_assistant_claimed_then_corrected_gap", "quoted_assistant_gap",
    {"claimed_verified_then_corrected"},
    [
        A('I told you "verified and passing" in the earlier note.'),
        U("Actually it broke again, that's wrong."),
    ],
    "KNOWN OPEN GAP: quoted verification language inside an assistant message is treated as the assistant's own live claim; combined with a later real correction this fires.",
    known_gap=True,
))

# ---------------------------------------------------------------------------
# D. TOOL-RESULT carried text — verification language that lives only in
#    tool output, never asserted by the assistant, must not fire. This
#    holds structurally (assistant-claim detectors filter on
#    EventKind::AssistantMessage). The moment the assistant *relays* that
#    tool text as its own sentence, the structural protection is gone and
#    the same quoting gap applies: KNOWN OPEN GAP.
# ---------------------------------------------------------------------------

CASES.append(case(
    "tool_carried_no_assistant_claim", "tool_carried",
    set(),
    [
        TC("run deployment script"),
        TR("Deployment log: verified and all set, tests passing, 0 failed."),
        A("Logged the output for review."),
    ],
    "Verification language lives only in tool output; the assistant never asserts it. Structurally protected by the AssistantMessage kind filter.",
))

CASES.append(case(
    "tool_carried_relayed_by_assistant_gap", "tool_carried_gap",
    {"verification_claim_without_tool_evidence"},
    [
        TC("run deployment script"),
        TR("Deployment log: verified and all set, tests passing."),
        A("The log says the deploy is verified and all set."),
    ],
    "KNOWN OPEN GAP: the assistant merely restates tool-output text, but the tool result does not match the narrow actual_test_result pattern, so the restated sentence reads as an unsupported, fresh verification claim.",
    known_gap=True,
))

# ---------------------------------------------------------------------------
# E. HYPOTHETICAL narration — user-authored hypotheticals are correctly
#    guarded (guard_user_span applies contains_hypothetical_narration);
#    assistant-authored hypotheticals are a KNOWN OPEN gap for the same
#    structural reason as quoting above.
# ---------------------------------------------------------------------------

CASES.append(case(
    "hypothetical_user_correctly_suppressed", "hypothetical_user",
    set(),
    [U("Hypothetically, if you were in the wrong repository, what would you do?")],
    "Hypothetical framing on a user message is suppressed by guard_user_span before the pattern is even evaluated.",
))

CASES.append(case(
    "hypothetical_assistant_verification_claim_gap", "hypothetical_assistant_gap",
    {"verification_claim_without_tool_evidence"},
    [A("If the deploy had gone smoothly, I would have called it verified and moved on.")],
    "KNOWN OPEN GAP: assistant counterfactual narration is read as a live verification claim because assistant_events() never applies contains_hypothetical_narration.",
    known_gap=True,
))

CASES.append(case(
    "hypothetical_assistant_claimed_then_corrected_gap", "hypothetical_assistant_gap",
    {"claimed_verified_then_corrected"},
    [
        A("If everything had gone right, I'd say it's verified and passing."),
        U("Actually it's still broken, that's not right."),
    ],
    "KNOWN OPEN GAP: same structural gap, now paired with a genuine later correction so the two-part detector fires.",
    known_gap=True,
))

# ---------------------------------------------------------------------------
# F. CROSS-SESSION duplicate detection
# ---------------------------------------------------------------------------

CASES.append(case(
    "cross_session_same_session_not_duplicate", "cross_session_negative",
    set(),
    [
        A("Applying the standard retry wrapper around the fetch call.", session_id="sess_only"),
        A("Applying the standard retry wrapper around the fetch call.", session_id="sess_only"),
    ],
    "Identical assistant text repeated within a single session is not a cross-session duplicate by design.",
))

CASES.append(case(
    "cross_session_distinct_text_not_duplicate", "cross_session_negative",
    set(),
    [
        A("Applying the retry wrapper around the fetch call.", session_id="xsession:c"),
        A("Applying a completely different caching strategy here.", session_id="xsession:d"),
    ],
    "Different text across different sessions is not a duplicate (sanity baseline).",
))

# ---------------------------------------------------------------------------
# Write cases.jsonl deterministically.
# ---------------------------------------------------------------------------


def main() -> None:
    seen = set()
    for c in CASES:
        if c["case_id"] in seen:
            raise SystemExit(f"duplicate case_id: {c['case_id']}")
        seen.add(c["case_id"])
        for i, ev in enumerate(c["events"]):
            ev["event_id"] = f"{c['case_id']}-{i}"

    out = HERE / "cases.jsonl"
    with out.open("w", encoding="utf-8") as f:
        for c in CASES:
            f.write(json.dumps(c, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
            f.write("\n")
    print(f"wrote {len(CASES)} cases to {out}")


if __name__ == "__main__":
    main()
