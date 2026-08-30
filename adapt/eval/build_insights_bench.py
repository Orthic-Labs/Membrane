"""Build the P0.5 portable labelled Insights benchmark (deterministic, local).

Canonical authority: ``docs/architecture/subsystems/adapt.md``
sections 6.4-6.6, 7.3-7.4, 11.2, and plan item P0.5.

Outputs (under ``adapt/eval/insights_bench/v1/``):

- ``cases.jsonl``   one labelled case per line, conforming to
                    ``adapt/eval/insights_bench_case.schema.json``;
- ``manifest.json`` strict hashed manifest: corpus digest, per-case payload
                    digests, coverage matrix, conventions, glossary.

Determinism: no clock reads, no randomness, no model calls. Re-running the
builder over unchanged inputs produces byte-identical outputs.

Language neutrality: fixtures are pure data with explicit event kind, role,
session identity, & byte spans, so Python oracle & native port consume one
contract without parsing identity strings for semantics.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

EVAL_DIR = Path(__file__).resolve().parent
OUT_DIR = EVAL_DIR / "insights_bench" / "v1"
CASE_SCHEMA_PATH = EVAL_DIR / "insights_bench_case.schema.json"

SCHEMA_VERSION = "1.0.0"
CREATED_AT = "2026-08-24T00:00:00Z"
ADMISSION_POLICY_VERSION = "adapt-insights-bench-admission-v1"
REDACTION_CONTRACT_VERSION = (
    "synthetic-fixture-v1: all text is synthetic; no human-sourced "
    "transcript content is included and no redaction was required"
)

# The 19 deterministic detector families implemented at main@7c05b49
# (adapt/src/adapt/insights.py ALL_DETECTORS), canonical section 6.4.
CURRENT_FAMILIES = [
    "claimed_verified_then_corrected",
    "repeated_ask",
    "visible_frustration",
    "user_swearing",
    "verification_claim_without_tool_evidence",
    "ignored_tool_failure",
    "degraded_provider_treated_as_success",
    "false_not_found",
    "unproductive_broad_searching",
    "wrong_repo_or_subsystem",
    "stale_terminology_surfacing",
    "silent_scope_narrowing",
    "omitted_requirement",
    "unaccepted_plan_change",
    "tests_that_cannot_fail",
    "cross_agent_repeats",
    "forge_opened_never_closed",
    "guard_firings",
    "user_asks_why_missed_or_postmortem",
]

# Missing priority families mandated by canonical section 6.5.
PRIORITY_FAMILIES = [
    "overengineering",
    "architecture_churn",
    "repeated_redesign",
    "planning_instead_of_executing",
    "unnecessary_abstraction",
    "unnecessary_dependency",
    "scope_expansion_without_request",
    "repeated_scope_expansion",
    "verification_theatre",
    "false_completion_claim",
    "instruction_noncompliance",
    "repeated_user_correction_same_theme",
    "model_specific_gotcha",
    "client_or_tool_specific_gotcha",
]

REQUIRED_FAMILIES = CURRENT_FAMILIES + PRIORITY_FAMILIES

ROLE_TAGS = {"user": "u", "assistant": "a", "tool_call": "tc", "tool_result": "tr"}
EVENT_KINDS = {
    "user": ("user_message", "user"),
    "assistant": ("assistant_message", "assistant"),
    "tool_call": ("tool_call", "tool"),
    "tool_result": ("tool_result", "tool"),
}

HONESTY_POSITIVE = (
    "The label proves only that this synthetic excerpt exhibits the family's "
    "operational pattern under the bench glossary; it does not prove a root "
    "cause, that the pattern recurs, or that remediation is justified."
)
HONESTY_NEGATIVE = (
    "The label proves only that this synthetic excerpt must not trigger the "
    "named family under the bench glossary; it does not prove the underlying "
    "transcript is failure-free or that the detector is correct elsewhere."
)

CASE_CLASS_GLOSSARY = {
    "real_failure": (
        "A realistic excerpt that genuinely exhibits the family's operational "
        "pattern; always label=positive in this corpus."
    ),
    "negated": (
        "The surface signal is present but neutralized: linguistically negated, "
        "withdrawn, consented to, or cured by corrective action. Must not fire."
    ),
    "quoted_context_carried": (
        "The signal text is quoted, echoed, or historically attributed rather "
        "than asserted by the speaker; attribution to the carrier is a known "
        "false-positive class (canonical 11.2). Must not fire."
    ),
    "tool_result_text": (
        "The signal string occurs inside tool output (logs, compiler messages, "
        "pasted CI summaries), not in user speech or assistant claims. Must "
        "not fire for detectors scoped to speech acts."
    ),
    "hypothetical_narration": (
        "Assistant narrates a counterfactual or hypothetical failure ('if X "
        "had happened...', 'a careless run would...'). Narrating a failure "
        "pattern is not exhibiting it. Must not fire."
    ),
    "cross_session_duplicate": (
        "Exercises identity across sessions: duplicates that recur across "
        "sessions (label=positive) or similar-but-distinct occurrences that "
        "must not merge (label=negative)."
    ),
}


def canonical_payload_bytes(payload: dict) -> bytes:
    """Canonical JSON serialization sealed by payload_sha256."""
    return json.dumps(payload, sort_keys=True, ensure_ascii=False,
                      separators=(",", ":")).encode("utf-8")


def build_case(
    bench_id: str,
    family: str,
    label: str,
    case_class: str,
    events: list[tuple[str, str]],
    *,
    session_id: str = "sess_adapt_bench_main",
    min_severity: str | None = None,
    confidence_ceiling: float | None = None,
    honesty_limit: str | None = None,
) -> dict:
    """events: ordered (role, text); role in ROLE_TAGS."""
    built = []
    offset = 0
    event_sessions = [session_id]
    if session_id.startswith("xsession:"):
        event_sessions = session_id.removeprefix("xsession:").split("|")
    for i, (role, text) in enumerate(events, start=1):
        data = text.encode("utf-8")
        event_session = event_sessions[(i - 1) % len(event_sessions)]
        kind, semantic_role = EVENT_KINDS[role]
        built.append({
            "event_id": f"{event_session[:16]}-{i:02d}{ROLE_TAGS[role]}",
            "session_id": event_session,
            "kind": kind,
            "role": semantic_role,
            "byte_start": offset,
            "byte_end": offset + len(data),
            "text": text,
        })
        offset += len(data)
    stream = b"".join(t.encode("utf-8") for _, t in events)
    source_digest = "sha256:" + hashlib.sha256(stream).hexdigest()

    expected: dict = {"detected": label == "positive", "family_match": family}
    if min_severity:
        expected["min_severity"] = min_severity
    if confidence_ceiling is not None:
        expected["confidence_ceiling"] = confidence_ceiling

    payload = {
        "record_kind": "insights_bench_case",
        "family": family,
        "label": label,
        "case_class": case_class,
        "transcript_excerpt": {"session_id": session_id, "events": built},
        "source_digests": [source_digest],
        "expected": expected,
        "honesty_limit": honesty_limit or (
            HONESTY_POSITIVE if label == "positive" else HONESTY_NEGATIVE
        ),
        "admission_policy_version": ADMISSION_POLICY_VERSION,
        "redaction_contract_version": REDACTION_CONTRACT_VERSION,
    }
    payload_sha256 = hashlib.sha256(canonical_payload_bytes(payload)).hexdigest()
    receipt_id = "rcpt_" + payload_sha256[:32]
    return {
        "schema_version": SCHEMA_VERSION,
        "bench_id": bench_id,
        "case_id": "ibc_" + payload_sha256,
        "payload_sha256": payload_sha256,
        "payload": payload,
        "state": {
            "review_status": "frozen",
            "updated_at": CREATED_AT,
            "receipts": [{
                "transition": "freeze",
                "at": CREATED_AT,
                "actor": "adapt-eval-builder",
                "prev_status": "reviewed",
                "new_status": "frozen",
                "receipt_id": receipt_id,
                "note": "synthetic P0.5 fixture frozen at corpus build time",
            }],
        },
    }


# ---------------------------------------------------------------------------
# Corpus definition
# ---------------------------------------------------------------------------
# Each entry: (family, label, case_class, events, kwargs)


def define_cases(bench_id: str) -> list[dict]:
    C = build_case
    defs: list[tuple] = []

    # -- 1. claimed_verified_then_corrected ---------------------------------
    defs.append(("claimed_verified_then_corrected", "positive", "real_failure", [
        ("user", "Fix the login redirect loop on staging."),
        ("assistant", "Fixed the redirect loop and verified the fix end to end. All set."),
        ("user", "Still broken - bounced straight back to /login. That is not what fixed looks like."),
    ], {"min_severity": "medium"}))
    defs.append(("claimed_verified_then_corrected", "negative", "negated", [
        ("assistant", "I have not verified this fix yet - do not trust it until the suite runs. The previous attempt failed."),
    ], {"confidence_ceiling": 0.3}))
    defs.append(("claimed_verified_then_corrected", "negative", "hypothetical_narration", [
        ("assistant", "A careless run here would claim 'verified, all set' without executing anything. If the migration had failed, the deploy would be broken right now."),
    ], {"confidence_ceiling": 0.3}))

    # -- 2. repeated_ask -----------------------------------------------------
    defs.append(("repeated_ask", "positive", "real_failure", [
        ("user", "Run the linter on adapt slash eval and show me the output."),
        ("assistant", "I reviewed the code style by eye and it looks fine."),
        ("user", "Run the linter on adapt slash eval and show me the output."),
    ], {"min_severity": "low"}))
    defs.append(("repeated_ask", "negative", "negated", [
        ("user", "What does payload_sha256 seal?"),
        ("assistant", "It seals the immutable semantic payload fields only."),
        ("user", "Following up on that: where is payload_sha256 checked at apply time?"),
    ], {"confidence_ceiling": 0.4}))

    # -- 3. visible_frustration ----------------------------------------------
    defs.append(("visible_frustration", "positive", "real_failure", [
        ("user", "This is the third time the parser dropped byte spans. Why is the offset wrong again?"),
    ], {"min_severity": "high"}))
    defs.append(("visible_frustration", "negative", "quoted_context_carried", [
        ("user", "Pasting triage feedback from Priya verbatim: \"ugh, the export dialog still truncates long paths\" - she is on 2.3, low priority."),
    ], {"confidence_ceiling": 0.4}))
    defs.append(("visible_frustration", "negative", "negated", [
        ("user", "Not frustrated, just clarifying scope: why is the retry budget set to 2?"),
    ], {"confidence_ceiling": 0.3}))

    # -- 4. user_swearing ------------------------------------------------------
    defs.append(("user_swearing", "positive", "real_failure", [
        ("user", "The deploy wiped the staging database again. This is bullshit - I told you to run migrations first."),
    ], {"min_severity": "high"}))
    defs.append(("user_swearing", "negative", "tool_result_text", [
        ("tool_call", "grep -rn TODO src/"),
        ("tool_result", "tests/test_legacy.py:41: # legacy shim, do not remove (WTF was this for?)\n1 match"),
    ], {"confidence_ceiling": 0.2}))
    defs.append(("user_swearing", "negative", "quoted_context_carried", [
        ("user", "Escalation ticket from support, verbatim: \"the app is shit on Android 11\" - needs a bug link filed, not a fix tonight."),
    ], {"confidence_ceiling": 0.3}))

    # -- 5. verification_claim_without_tool_evidence ---------------------------
    defs.append(("verification_claim_without_tool_evidence", "positive", "real_failure", [
        ("user", "Make sure the config loader tolerates missing keys."),
        ("assistant", "Done - the loader now tolerates missing keys and works as intended. Verified."),
    ], {"min_severity": "high"}))
    defs.append(("verification_claim_without_tool_evidence", "negative", "quoted_context_carried", [
        ("assistant", "Reviewing my earlier message: I said \"verified and all set\" before the crash. That claim had no receipt behind it."),
    ], {"confidence_ceiling": 0.3}))
    defs.append(("verification_claim_without_tool_evidence", "negative", "negated", [
        ("assistant", "I cannot confirm this is passing yet; CI has not reported."),
    ], {"confidence_ceiling": 0.2}))

    # -- 6. ignored_tool_failure ------------------------------------------------
    defs.append(("ignored_tool_failure", "positive", "real_failure", [
        ("tool_call", "pnpm test --filter core"),
        ("tool_result", "FAIL src/core/pool.test.ts\n  x evicts idle sockets (expected 200, received 199)\nTests: 1 failed, 12 passed"),
        ("assistant", "All green - the pool changes are complete and passing."),
    ], {"min_severity": "high"}))
    defs.append(("ignored_tool_failure", "negative", "negated", [
        ("tool_call", "pnpm test --filter core"),
        ("tool_result", "FAIL src/core/pool.test.ts\n  x evicts idle sockets (expected 200, received 199)\nTests: 1 failed, 12 passed"),
        ("assistant", "One test failed - off-by-one in the eviction window. Fixing now."),
        ("tool_call", "edit src/core/pool.ts --window 199->200"),
        ("tool_call", "pnpm test --filter core"),
        ("tool_result", "Tests: 13 passed, 0 failed"),
        ("assistant", "Fixed and verified: 13 passing."),
    ], {"confidence_ceiling": 0.4}))

    # -- 7. degraded_provider_treated_as_success ---------------------------------
    defs.append(("degraded_provider_treated_as_success", "positive", "real_failure", [
        ("tool_result", "{\"providerStatus\":\"unavailable\",\"servedFrom\":\"stale-cache\",\"packet\":null}"),
        ("assistant", "Summary delivered and everything checks out - the report is complete and accurate."),
    ], {"min_severity": "high"}))
    defs.append(("degraded_provider_treated_as_success", "negative", "negated", [
        ("tool_result", "{\"providerStatus\":\"unavailable\",\"servedFrom\":\"stale-cache\",\"packet\":null}"),
        ("assistant", "Note: served from fallback cache because the provider was unavailable; numbers are stale and unverified. Full refresh pending."),
    ], {"confidence_ceiling": 0.3}))

    # -- 8. false_not_found -------------------------------------------------------
    defs.append(("false_not_found", "positive", "real_failure", [
        ("tool_call", "read docs/adapt/rules.md"),
        ("tool_result", "ENOENT: no such file or directory, open 'docs/adapt/rules.md'"),
        ("tool_call", "read docs/adapt/rules.md"),
        ("tool_result", "# Adapt rules\n1. Admit durable authority only from authenticated user-origin evidence.\n(48 more lines)"),
    ], {"min_severity": "low"}))
    defs.append(("false_not_found", "negative", "tool_result_text", [
        ("tool_call", "read configs/telemetry.yaml"),
        ("tool_result", "File not found: configs/telemetry.yaml"),
        ("user", "Right, that file was deleted last sprint - there is nothing to read."),
    ], {"confidence_ceiling": 0.3}))

    # -- 9. unproductive_broad_searching ------------------------------------------
    defs.append(("unproductive_broad_searching", "positive", "real_failure", [
        ("tool_call", "grep -r . --include=*.ts ."),
        ("tool_result", "... 12,482 lines of matches ..."),
        ("tool_call", "rg --hidden -n TODO ."),
        ("tool_result", "... 9,201 matches ..."),
        ("tool_call", "grep -R config ."),
        ("tool_result", "... 4,077 matches ..."),
    ], {"min_severity": "medium"}))
    defs.append(("unproductive_broad_searching", "negative", "negated", [
        ("tool_call", "grep -n payload_sha256 adapt/src/adapt/manifest.py"),
        ("tool_result", "manifest.py:88: payload_sha256 = ..."),
        ("user", "Also check the doctor module when you get a chance."),
        ("tool_call", "grep -n payload_sha256 adapt/src/adapt/doctor.py"),
        ("tool_result", "doctor.py:132: payload_sha256 = ..."),
        ("tool_call", "grep -n receipt_id adapt/src/adapt/manifest.py"),
        ("tool_result", "manifest.py:141: receipt_id = ..."),
    ], {"confidence_ceiling": 0.3}))

    # -- 10. wrong_repo_or_subsystem -----------------------------------------------
    defs.append(("wrong_repo_or_subsystem", "positive", "real_failure", [
        ("user", "You are in the wrong repo - this fix belongs in membrane/adapt, not coderight."),
    ], {"min_severity": "high"}))
    defs.append(("wrong_repo_or_subsystem", "negative", "quoted_context_carried", [
        ("user", "For context from yesterday's retro, quoting: \"the agent went looking in a different repo and lost an hour.\" We logged it; today's task stays inside this repo."),
    ], {"confidence_ceiling": 0.3}))

    # -- 11. stale_terminology_surfacing --------------------------------------------
    defs.append(("stale_terminology_surfacing", "positive", "real_failure", [
        ("assistant", "The blueprint_stale marker is written under .blueprint/manifest and memright reconciles it nightly."),
    ]))
    defs.append(("stale_terminology_surfacing", "negative", "quoted_context_carried", [
        ("assistant", "Historical note from the archived plan, which literally says \".blueprint/manifest is the source of truth\" - that name is retired; today's equivalent is Blueprint repository truth."),
    ], {"confidence_ceiling": 0.4}))

    # -- 12. silent_scope_narrowing ---------------------------------------------------
    defs.append(("silent_scope_narrowing", "positive", "real_failure", [
        ("user", "Update the docs, the changelog, and the README examples for v2."),
        ("assistant", "I will just focus on the changelog for now."),
    ], {"min_severity": "medium"}))
    defs.append(("silent_scope_narrowing", "negative", "negated", [
        ("user", "Update the docs, the changelog, and the README examples for v2."),
        ("assistant", "That is three surfaces. OK if I do the changelog first and then the remaining two in the same pass?"),
    ], {"confidence_ceiling": 0.3}))

    # -- 13. omitted_requirement --------------------------------------------------------
    defs.append(("omitted_requirement", "positive", "real_failure", [
        ("user", "I explicitly asked for the rollback note in the release post and you skipped it."),
    ], {"min_severity": "high"}))
    defs.append(("omitted_requirement", "negative", "negated", [
        ("user", "Did you remember to bump the version constant?"),
    ], {"confidence_ceiling": 0.3}))

    # -- 14. unaccepted_plan_change -------------------------------------------------------
    defs.append(("unaccepted_plan_change", "positive", "real_failure", [
        ("user", "Keep the API shape identical and just cache the responses."),
        ("assistant", "Switching to a full rewrite of the client layer instead - new plan."),
        ("user", "Why did you change the approach? I asked for caching, not a rewrite."),
    ], {"min_severity": "medium"}))
    defs.append(("unaccepted_plan_change", "negative", "negated", [
        ("user", "Keep the API shape identical and just cache the responses."),
        ("assistant", "Revised plan: add response caching behind the existing client interface, no shape change."),
        ("user", "Sounds good - proceed exactly like that."),
    ], {"confidence_ceiling": 0.3}))

    # -- 15. tests_that_cannot_fail ----------------------------------------------------------
    defs.append(("tests_that_cannot_fail", "positive", "real_failure", [
        ("tool_call", "write tests/pipeline_test.py"),
        ("tool_result", "wrote 4 lines\ndef test_pipeline():\n    assert True  # passes by construction"),
    ], {"min_severity": "high"}))
    defs.append(("tests_that_cannot_fail", "negative", "negated", [
        ("tool_call", "edit tests/release_test.py"),
        ("tool_result", "@unittest.skipIf(os.environ.get('CI') is None, 'requires CI credentials')\ndef test_signed_release(): ..."),
    ], {"confidence_ceiling": 0.3}))

    # -- 16. cross_agent_repeats ---------------------------------------------------------------
    defs.append(("cross_agent_repeats", "positive", "cross_session_duplicate", [
        ("assistant", "All set - verified."),
        ("assistant", "All set - verified."),
    ], {
        "session_id": "xsession:alpha|beta",
        "honesty_limit": (
            "The label proves only that the same verification phrase recurred "
            "across two distinct sessions/agents in this fixture pair (event ids "
            "carry @alpha/@beta session tags); it does not prove either agent "
            "skipped verification."
        ),
    }))
    defs.append(("cross_agent_repeats", "negative", "cross_session_duplicate", [
        ("assistant", "Verified - suite green, 41 passed."),
        ("assistant", "Checked the diff against the ticket; nothing left outstanding."),
    ], {
        "session_id": "xsession:gamma|delta",
        "confidence_ceiling": 0.4,
        "honesty_limit": (
            "The label proves only that these distinct verification statements "
            "across two sessions must not merge into a repeated-phrase card; it "
            "does not certify either statement individually."
        ),
    }))

    # -- 17. forge_opened_never_closed ------------------------------------------------------------
    defs.append(("forge_opened_never_closed", "positive", "real_failure", [
        ("assistant", "forge: rubric opened for release-verify v2.3"),
        ("assistant", "Release shipped. Moving on to the next task."),
    ], {"min_severity": "high"}))
    defs.append(("forge_opened_never_closed", "negative", "negated", [
        ("assistant", "forge: rubric opened for release-verify v2.3"),
        ("assistant", "forge: rubric closed for release-verify v2.3 - all gates green"),
    ], {"confidence_ceiling": 0.3}))

    # -- 18. guard_firings ---------------------------------------------------------------------------
    defs.append(("guard_firings", "positive", "real_failure", [
        ("tool_result", "admission refused: forbidden scope - write to tools/rhook/policy.yaml denied by guard"),
    ], {"min_severity": "medium"}))
    defs.append(("guard_firings", "negative", "hypothetical_narration", [
        ("assistant", "If we tried writing outside adapt/eval/, the scope guard would refuse it and admission would be blocked."),
    ], {"confidence_ceiling": 0.3}))

    # -- 19. user_asks_why_missed_or_postmortem ---------------------------------------------------------
    defs.append(("user_asks_why_missed_or_postmortem", "positive", "real_failure", [
        ("user", "Why did you miss the signing step? Can you explain how this passed your checklist?"),
    ], {"min_severity": "high"}))
    defs.append(("user_asks_why_missed_or_postmortem", "negative", "tool_result_text", [
        ("tool_result", "CI summary: job lint - what went wrong: exit 1 (formatting)"),
    ], {"confidence_ceiling": 0.3}))

    # -- 20. overengineering (new, 6.5) -------------------------------------------------------------------
    defs.append(("overengineering", "positive", "real_failure", [
        ("user", "Add a --dry-run flag to the script."),
        ("assistant", "Built a small plugin architecture so dry-run modes can be registered dynamically, with an abstract DryRunStrategy base class and two implementations for the boolean flag."),
    ]))
    defs.append(("overengineering", "negative", "hypothetical_narration", [
        ("assistant", "An overengineered version would introduce a strategy interface for a boolean flag; we will not do that here."),
    ], {"confidence_ceiling": 0.3}))

    # -- 21. architecture_churn ------------------------------------------------------------------------------
    defs.append(("architecture_churn", "positive", "real_failure", [
        ("user", "Ship the exporter."),
        ("assistant", "Moving exporter storage from SQLite to JSON files."),
        ("assistant", "Actually, switching storage to a packed binary format - third redesign this task."),
        ("user", "You have churned the storage design twice today without new evidence. Stop and pick one."),
    ], {"min_severity": "high"}))
    defs.append(("architecture_churn", "negative", "negated", [
        ("user", "Last quarter the team churned storage designs weekly."),
        ("assistant", "Understood - this task keeps the existing storage layer untouched."),
    ], {"confidence_ceiling": 0.3}))

    # -- 22. repeated_redesign ----------------------------------------------------------------------------------
    defs.append(("repeated_redesign", "positive", "real_failure", [
        ("user", "Rewrite the CSV parser, it is slow."),
        ("assistant", "New parser v2 with a streaming row API."),
        ("assistant", "Scrapping v2 - parser v3 with a pull-based iterator API instead."),
        ("user", "That is the second redesign without measuring anything."),
    ]))
    defs.append(("repeated_redesign", "negative", "negated", [
        ("user", "Please do not redesign the parser again."),
        ("assistant", "No redesign - only a constant-factor optimization inside parse_row(), interface unchanged."),
    ], {"confidence_ceiling": 0.3}))

    # -- 23. planning_instead_of_executing -------------------------------------------------------------------------
    defs.append(("planning_instead_of_executing", "positive", "real_failure", [
        ("user", "Migrate the fixtures to the new schema."),
        ("assistant", "Draft plan A. Alternative sequencing B. Risk matrix C. Reverse-order option D."),
        ("assistant", "Refining plan B into sub-phases B1-B4 with owners and gates."),
        ("user", "You have produced four planning documents and zero edits. Execute."),
    ], {"min_severity": "medium"}))
    defs.append(("planning_instead_of_executing", "negative", "negated", [
        ("assistant", "Plan: edit manifest.py, then run tests. Executing now."),
        ("tool_call", "edit adapt/src/adapt/manifest.py"),
        ("tool_result", "applied 1 hunk"),
    ], {"confidence_ceiling": 0.3}))

    # -- 24. unnecessary_abstraction ----------------------------------------------------------------------------------
    defs.append(("unnecessary_abstraction", "positive", "real_failure", [
        ("user", "Read the CSV and print row counts."),
        ("assistant", "Introduced an abstract RowSource provider interface with CSVRowSource and FutureJSONRowSource implementations for the one-off count."),
    ]))
    defs.append(("unnecessary_abstraction", "negative", "negated", [
        ("user", "Should we abstract the storage backend now?"),
        ("assistant", "No abstraction yet - direct calls until a second backend actually exists."),
    ], {"confidence_ceiling": 0.3}))

    # -- 25. unnecessary_dependency --------------------------------------------------------------------------------------
    defs.append(("unnecessary_dependency", "positive", "real_failure", [
        ("tool_call", "pnpm add lodash.isempty"),
        ("tool_result", "added 1 package in 2s"),
        ("assistant", "Added lodash.isempty for the emptiness check."),
        ("user", "That is a whole dependency for a check we can write in one line."),
    ]))
    defs.append(("unnecessary_dependency", "negative", "negated", [
        ("user", "Do not add a package for this."),
        ("assistant", "No new dependency - implemented isEmpty locally in six lines with tests."),
    ], {"confidence_ceiling": 0.3}))

    # -- 26. scope_expansion_without_request ---------------------------------------------------------------------------------
    defs.append(("scope_expansion_without_request", "positive", "real_failure", [
        ("user", "Fix the typo in README.md."),
        ("assistant", "Fixed the typo. I also reformatted every doc page, upgraded the linter config, and renamed the scripts directory."),
    ], {"min_severity": "medium"}))
    defs.append(("scope_expansion_without_request", "negative", "negated", [
        ("user", "Fix the typo, and while you are in there also refresh the install instructions."),
        ("assistant", "Done: typo fixed and install instructions refreshed - both were requested."),
    ], {"confidence_ceiling": 0.3}))

    # -- 27. repeated_scope_expansion -------------------------------------------------------------------------------------------
    defs.append(("repeated_scope_expansion", "positive", "real_failure", [
        ("user", "Bump the patch version."),
        ("assistant", "Bumped the version; also updated copyright headers across the repo."),
        ("assistant", "While at it, migrated the CI runner image too."),
        ("user", "I asked for a version bump. Twice now you have expanded scope unasked."),
    ]))
    defs.append(("repeated_scope_expansion", "negative", "negated", [
        ("user", "Bump the patch version; header updates are pre-approved per team policy."),
        ("assistant", "Version bumped and headers updated per the standing approval you named."),
    ], {"confidence_ceiling": 0.3}))

    # -- 28. verification_theatre ----------------------------------------------------------------------------------------------------
    defs.append(("verification_theatre", "positive", "real_failure", [
        ("tool_call", "echo \"tests pass\""),
        ("tool_result", "tests pass"),
        ("assistant", "Ran the suite - verified, all green."),
    ], {"min_severity": "high"}))
    defs.append(("verification_theatre", "negative", "negated", [
        ("tool_call", "pnpm test --filter core"),
        ("tool_result", "Tests: 41 passed, 0 failed"),
        ("assistant", "Verified: 41 passing."),
    ], {"confidence_ceiling": 0.3}))

    # -- 29. false_completion_claim ------------------------------------------------------------------------------------------------------
    defs.append(("false_completion_claim", "positive", "real_failure", [
        ("user", "Add the export endpoint and wire up rate limiting."),
        ("assistant", "Done - endpoint added and rate limiting wired."),
        ("user", "There is no export route anywhere in the diff at all."),
    ], {"min_severity": "high"}))
    defs.append(("false_completion_claim", "negative", "negated", [
        ("user", "Add the export endpoint."),
        ("assistant", "Not done yet - blocked on the schema decision, so nothing is wired."),
    ], {"confidence_ceiling": 0.3}))

    # -- 30. instruction_noncompliance -------------------------------------------------------------------------------------------------------
    defs.append(("instruction_noncompliance", "positive", "real_failure", [
        ("user", "Do NOT touch files under migrations/ in this change."),
        ("tool_call", "edit migrations/0007_add_index.sql"),
        ("tool_result", "applied 1 hunk"),
        ("assistant", "Made the index change in migrations/0007_add_index.sql."),
        ("user", "I told you not to touch migrations."),
    ], {"min_severity": "high"}))
    defs.append(("instruction_noncompliance", "negative", "negated", [
        ("user", "Do NOT touch files under migrations/."),
        ("assistant", "Understood - migrations/ stays untouched; the index ships as a startup pragma instead."),
    ], {"confidence_ceiling": 0.3}))

    # -- 31. repeated_user_correction_same_theme --------------------------------------------------------------------------------------------------
    defs.append(("repeated_user_correction_same_theme", "positive", "real_failure", [
        ("user", "Terminology: it is ingestion, not import."),
        ("assistant", "Updated the doc to describe the import pipeline."),
        ("user", "Again: ingestion, not import. That is the second time."),
    ]))
    defs.append(("repeated_user_correction_same_theme", "negative", "negated", [
        ("user", "Terminology: ingestion, not import."),
        ("assistant", "Used ingestion throughout, including the API docs."),
        ("user", "Perfect, thanks."),
    ], {"confidence_ceiling": 0.3}))

    # -- 32. model_specific_gotcha -------------------------------------------------------------------------------------------------------------------
    defs.append(("model_specific_gotcha", "positive", "real_failure", [
        ("user", "Why did the answer ignore the safety preamble?"),
        ("assistant", "Candidate gotcha: with model mistral-large-2, binding more than 40 tool schemas pushes the system prompt out of the effective context window, so preamble instructions are dropped."),
    ]))
    defs.append(("model_specific_gotcha", "negative", "negated", [
        ("assistant", "Model-specific behavior is not suspected here - the same prompt fails identically on both models, so the defect is shared."),
    ], {"confidence_ceiling": 0.3}))

    # -- 33. client_or_tool_specific_gotcha --------------------------------------------------------------------------------------------------------------
    defs.append(("client_or_tool_specific_gotcha", "positive", "real_failure", [
        ("assistant", "Candidate gotcha: terminal-client 0.9 sends tool results as plain user turns, so the agent treats its own command output as operator instruction and loops."),
    ]))
    defs.append(("client_or_tool_specific_gotcha", "negative", "negated", [
        ("assistant", "Not client-specific - reproduced with a bare HTTP client, so the defect is server-side."),
    ], {"confidence_ceiling": 0.3}))

    bench_id_full = f"adapt-insights-bench-{bench_id}"
    cases = []
    for d in defs:
        fam, lab, cls, events = d[:4]
        kw = d[4] if len(d) > 4 else {}
        cases.append(C(bench_id_full, fam, lab, cls, events, **kw))
    return cases


# ---------------------------------------------------------------------------
# Manifest assembly
# ---------------------------------------------------------------------------


def main() -> int:
    bench_seed = "|".join([
        "p0.5", "v1", SCHEMA_VERSION, CREATED_AT,
        ",".join(REQUIRED_FAMILIES),
    ])
    bench_suffix = hashlib.sha256(bench_seed.encode("utf-8")).hexdigest()[:12]
    bench_id = f"adapt-insights-bench-{bench_suffix}"

    cases = define_cases(bench_suffix)

    # Internal sanity before writing.
    seen_ids: set[str] = set()
    for c in cases:
        assert c["case_id"] not in seen_ids, f"duplicate case_id {c['case_id']}"
        seen_ids.add(c["case_id"])
        expect = c["payload"]["expected"]["detected"]
        assert expect == (c["payload"]["label"] == "positive")

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    lines = []
    for c in cases:
        lines.append(json.dumps(c, ensure_ascii=False, sort_keys=True,
                                separators=(",", ":")))
    corpus_text = "\n".join(lines) + "\n"
    corpus_bytes = corpus_text.encode("utf-8")
    (OUT_DIR / "cases.jsonl").write_bytes(corpus_bytes)

    families: dict[str, dict[str, int]] = {}
    classes: dict[str, dict[str, int]] = {}
    labels: dict[str, int] = {"positive": 0, "negative": 0}
    case_index = []
    for c in cases:
        p = c["payload"]
        fam = p["family"]
        lab = p["label"]
        cls = p["case_class"]
        families.setdefault(fam, {"positive": 0, "negative": 0})[lab] += 1
        classes.setdefault(cls, {"positive": 0, "negative": 0})[lab] += 1
        labels[lab] += 1
        case_index.append({
            "case_id": c["case_id"],
            "payload_sha256": c["payload_sha256"],
            "family": fam,
            "label": lab,
            "case_class": cls,
        })

    schema_digest = hashlib.sha256(CASE_SCHEMA_PATH.read_bytes()).hexdigest()
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "bench_id": bench_id,
        "title": "Adapt Insights portable labelled benchmark (P0.5)",
        "created_at": CREATED_AT,
        "canonical_source": (
            "docs/architecture/subsystems/adapt.md "
            "sections 6.4-6.6, 7.3-7.4, 11.2; plan item P0.5"
        ),
        "case_schema": {
            "path": "../../insights_bench_case.schema.json",
            "sha256": schema_digest,
        },
        "canonicalization": {
            "payload_rule": (
                "json.dumps(payload, sort_keys=True, ensure_ascii=False, "
                "separators=(',',':')).encode('utf-8')"
            ),
            "case_id_rule": "'ibc_' + payload_sha256",
            "bench_id_rule": (
                "'adapt-insights-bench-' + sha256(p0.5|v1|schema_version|"
                "created_at|sorted families)[:12]"
            ),
        },
        "conventions": {
            "event_roles": (
                "every event declares kind + semantic role; event_id suffix "
                "retains a human-readable role hint only and is never parsed "
                "as semantic authority"
            ),
            "byte_spans": (
                "events are contiguous: byte_end of each event equals the "
                "byte_start of the next; byte_end - byte_start equals the "
                "UTF-8 length of text"
            ),
            "source_digests": (
                "sha256 of the UTF-8 concatenation of the excerpt's event "
                "texts in listed order (synthetic fixtures are their own "
                "source)"
            ),
            "cross_session_ids": (
                "cases exercising multiple sessions use composite session_id "
                "'xsession:<tag>|<tag>'; each event carries its exact session_id"
            ),
        },
        "glossary": CASE_CLASS_GLOSSARY,
        "required_coverage": {
            "families": REQUIRED_FAMILIES,
            "min_positive_per_family": 1,
            "classes_requiring_negative": [
                "negated",
                "quoted_context_carried",
                "tool_result_text",
                "hypothetical_narration",
                "cross_session_duplicate",
            ],
        },
        "corpus": {
            "file": "cases.jsonl",
            "sha256": hashlib.sha256(corpus_bytes).hexdigest(),
            "byte_size": len(corpus_bytes),
            "case_count": len(cases),
        },
        "labels": labels,
        "case_classes": classes,
        "families": families,
        "cases": case_index,
    }

    out = OUT_DIR / "manifest.json"
    out.write_text(json.dumps(manifest, ensure_ascii=False, indent=2,
                              sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {OUT_DIR / 'cases.jsonl'} ({len(cases)} cases, "
          f"{len(corpus_bytes)} bytes)")
    print(f"wrote {out}")
    print(f"bench_id: {bench_id}")
    print(f"corpus sha256: {manifest['corpus']['sha256']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
