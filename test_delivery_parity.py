from __future__ import annotations

import importlib.util
import json
import sqlite3
from pathlib import Path


ADAPT_DIR = Path(__file__).resolve().parent
BUILDER = ADAPT_DIR / "eval" / "build_delivery_parity_set.py"
RUNNER = ADAPT_DIR / "eval" / "run_delivery_parity.py"
EVIDENCE_RECALL = ADAPT_DIR / "eval" / "evaluate_delivery_evidence_recall.py"
GRADER_COMPARE = ADAPT_DIR / "eval" / "compare_delivery_graders.py"
BUDGETED = ADAPT_DIR / "eval" / "run_budgeted_delivery_hybrid.py"
CORE_BUILDER = ADAPT_DIR / "eval" / "build_budgeted_core_digest.py"
CONTROL_AUDIT = ADAPT_DIR / "eval" / "audit_delivery_controls.py"
ALIAS_RETRIEVAL = ADAPT_DIR / "eval" / "run_evidence_alias_retrieval.py"
ALIAS_HYBRID = ADAPT_DIR / "eval" / "run_alias_hybrid.py"

import pytest

# The delivery-parity fixtures are a machine-local frozen evidence pack under
# .cache/ (never committed). On a checkout without the pack these tests skip
# with a named reason instead of failing — same portability contract as the
# scanner-dependent extraction test. Evidence pack:
# docs/baselines/adapt-taste-parity-2026-07-14/README.md
_WS = ADAPT_DIR.parents[3]
_FROZEN_VALUE_INPUTS = _WS / ".cache/adapt-commandcode-delta-m3-seeded-sanitized-split1-v1/results-rescoped.json"
_FROZEN_TREATMENT = _WS / ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json"
requires_frozen_inputs = pytest.mark.skipif(
    not _FROZEN_VALUE_INPUTS.exists(),
    reason="machine-local frozen evidence pack absent (.cache/adapt-commandcode-delta-m3-seeded-sanitized-split1-v1)",
)
requires_frozen_treatment = pytest.mark.skipif(
    not _FROZEN_TREATMENT.exists(),
    reason="machine-local frozen delivery-parity treatment absent (.cache/adapt-delivery-parity/full/frozen)",
)


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@requires_frozen_inputs
def test_real_value_set_is_balanced_frozen_and_has_no_rule_leakage():
    module = _load(BUILDER, "build_delivery_parity_set")
    value_set, treatment = module.build_value_set(module.DEFAULT_TASTE, module.DEFAULT_ADAPT)

    assert value_set["counts"] == {
        "total": 60, "shared": 16, "taste_only": 16,
        "adapt_only": 16, "control": 12,
    }
    assert len(value_set["taste_rules"]) == 52
    assert len(treatment["records"]) == 57
    assert len(treatment["curated_records"]) == 49
    assert len(treatment["exclusions"]) == 8
    assert len({row["case_id"] for row in value_set["cases"]}) == 60
    for row in value_set["cases"]:
        expected = row.get("expected") or {}
        for key in ("taste_rule", "adapt_rule"):
            rule = expected.get(key)
            assert not rule or rule.casefold() not in row["prompt"].casefold()


@requires_frozen_inputs
def test_treatment_uses_raw_output_and_keeps_curation_as_secondary_audit():
    module = _load(BUILDER, "build_delivery_parity_set")
    _, treatment = module.build_value_set(module.DEFAULT_TASTE, module.DEFAULT_ADAPT)
    accepted = {row["id"] for row in treatment["records"]}
    curated = {row["id"] for row in treatment["curated_records"]}
    excluded = {row["id"] for row in treatment["exclusions"]}

    assert excluded == set(module.EXCLUSIONS)
    assert excluded <= accepted
    assert not curated & excluded
    assert "taste-workflow-af3b148e41" in accepted
    assert "taste-architecture-7532d5593c" in accepted
    assert all(row["status"] == "accepted" for row in treatment["records"])


def test_batched_replay_uses_one_replay_and_direct_sqlite_lookup(tmp_path, monkeypatch):
    module = _load(RUNNER, "run_delivery_parity")
    db = tmp_path / "arm.db"
    with sqlite3.connect(db) as conn:
        conn.execute("CREATE TABLE memories (id TEXT PRIMARY KEY, content TEXT, scope_id TEXT)")
        conn.execute("INSERT INTO memories VALUES ('D--Claude/m1', 'body one', 'D--Claude')")
    calls = []

    def fake_run(_memright, _db, _port, cmd, input_text=None):
        calls.append((cmd, input_text))
        return "\n".join([
            json.dumps({"row_id": "c1", "ranked_ids": ["D--Claude/m1"]}),
            json.dumps({"row_id": "c2", "ranked_ids": ["D--Claude/m1"]}),
        ])

    monkeypatch.setattr(module, "_run_via_port", fake_run)
    rows = [
        {"case_id": "c1", "prompt": "one", "scope": "D--Claude"},
        {"case_id": "c2", "prompt": "two", "scope": "D--Claude"},
    ]
    result = module.replay_all(Path("memright"), db, 1234, rows, tmp_path / "in.jsonl")

    assert len(calls) == 1
    assert calls[0][0][0] == "replay"
    assert result["c1"]["ranked_texts"] == ["body one"]
    assert result["c2"]["ranked_scopes"] == ["D--Claude"]


@requires_frozen_inputs
def test_smoke_selector_covers_every_positive_cohort_and_controls():
    runner = _load(RUNNER, "run_delivery_parity")
    builder = _load(BUILDER, "build_delivery_parity_set_for_smoke")
    value_set, _ = builder.build_value_set(builder.DEFAULT_TASTE, builder.DEFAULT_ADAPT)
    smoke = runner.select_cases(value_set["cases"], smoke=True)

    assert len(smoke) == 5
    assert {row["cohort"] for row in smoke} == {
        "shared", "taste_only", "adapt_only", "control",
    }


def test_actor_splits_a_truncated_batch_and_preserves_exact_coverage(tmp_path, monkeypatch):
    runner = _load(RUNNER, "run_delivery_parity_split")
    cases = [{"case_id": f"c{i}", "prompt": f"task {i}", "scope": "D--Claude"}
             for i in range(6)]
    retrieval = {
        "input_sha256": "r", "A": {c["case_id"]: {"ranked_texts": [], "ranked_scopes": []}
                                     for c in cases},
        "B": {c["case_id"]: {"ranked_texts": [], "ranked_scopes": []} for c in cases},
    }
    value_set = {"taste_rules": [], "content_sha256": "v"}
    treatment = {"records": []}

    monkeypatch.setattr(runner, "ARMS", ("A",))
    monkeypatch.setattr(runner.adapt_sessions, "scan_batch_for_secrets_str", lambda _x: True)

    def fake_call(system, user, **_kwargs):
        if system == runner.JSON_REPAIR_SYSTEM:
            return {"text": "[truncated", "model": "fake"}
        ids = [row["case_id"] for row in json.loads(user)["cases"]]
        if len(ids) > 5:
            return {"text": "[truncated", "model": "fake"}
        return {"text": json.dumps([{"case_id": cid, "answer": "ok"} for cid in ids]),
                "model": "fake"}

    monkeypatch.setattr(runner.adapt_llm, "call_lane_response", fake_call)
    result = runner.run_actor(value_set, treatment, cases, retrieval, tmp_path,
                              resume=False, batch_size=6)

    assert {row["case_id"] for row in result["answers"]} == {f"c{i}" for i in range(6)}
    assert (tmp_path / "actor" / "A-01a.json").exists()
    assert (tmp_path / "actor" / "A-01b.json").exists()


def test_grade_validator_rejects_nulls_and_accepts_complete_control_score():
    runner = _load(RUNNER, "run_delivery_parity_grade_validator")

    assert not runner.valid_grade({
        "adherence": None, "task_correct": None, "intrusion": False})
    assert runner.valid_grade({
        "adherence": 0, "task_correct": 2, "intrusion": False})


def test_paired_stats_reports_direction_and_exact_discordance():
    runner = _load(RUNNER, "run_delivery_parity_paired")
    cases = [{"case_id": "x", "cohort": "shared"},
             {"case_id": "y", "cohort": "adapt_only"}]
    grades = [
        {"arm": "A", "case_id": "x", "adherence": 0},
        {"arm": "D", "case_id": "x", "adherence": 2},
        {"arm": "A", "case_id": "y", "adherence": 1},
        {"arm": "D", "case_id": "y", "adherence": 2},
    ]
    result = runner.paired_arm_stats(grades, cases, "A", "D", samples=1000)

    assert result["mean_adherence_delta_pp"] == 75.0
    assert result["full_adherence_discordance"]["right_only"] == 2
    assert result["full_adherence_discordance"]["left_only"] == 0


@requires_frozen_inputs
def test_adapt_treatment_variants_are_explicit_and_hashed():
    runner = _load(RUNNER, "run_delivery_parity_variants")
    builder = _load(BUILDER, "build_delivery_parity_variants")
    _, source = builder.build_value_set(builder.DEFAULT_TASTE, builder.DEFAULT_ADAPT)

    raw = runner.select_adapt_treatment(source, "raw")
    curated = runner.select_adapt_treatment(source, "curated")

    assert raw["selected_record_count"] == 57
    assert curated["selected_record_count"] == 49
    assert raw["records_sha256"] != curated["records_sha256"]


def test_evidence_recall_scores_self_and_source_queries_separately():
    module = _load(EVIDENCE_RECALL, "evaluate_delivery_evidence_recall_test")
    cases = [
        {"case_id": "s", "kind": "rule_self", "expected_id": "x/r",
         "record_id": "r", "scope": "x", "category": "workflow",
         "record_type": "standing_preference", "prompt": "rule"},
        {"case_id": "e", "kind": "source_evidence", "expected_id": "x/r",
         "record_id": "r", "scope": "x", "category": "workflow",
         "record_type": "standing_preference", "prompt": "evidence"},
    ]
    ranked = {"s": {"ranked_ids": ["x/r"]}, "e": {"ranked_ids": ["other"]}}
    result = module.score_cases(cases, ranked)

    assert result["metrics"]["rule_self"]["hit_at_5_pct"] == 100.0
    assert result["metrics"]["source_evidence"]["hit_at_5_pct"] == 0.0


def test_weighted_kappa_is_one_for_identical_scores():
    module = _load(GRADER_COMPARE, "compare_delivery_graders_test")
    assert module.weighted_kappa([0, 1, 2], [0, 1, 2]) == 1.0


@requires_frozen_treatment
def test_budgeted_core_is_the_13_inherited_rules_and_fits_800_tokens():
    module = _load(BUDGETED, "run_budgeted_delivery_hybrid_test")
    treatment = json.loads((ADAPT_DIR.parents[3] /
                            ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json")
                           .read_text(encoding="utf-8"))
    core, block, tokens = module.core_treatment(treatment)

    assert len(core["records"]) == 13
    assert tokens <= 800
    assert len(block) > 0


def test_compiled_core_validator_enforces_sources_and_budget():
    module = _load(CORE_BUILDER, "build_budgeted_core_digest_test")
    payload = {"rules": [
        {"text": f"Keep broad workflow preference {i}.", "source_ids": [f"s{i}"]}
        for i in range(6)
    ]}
    block, tokens = module.validate(payload, {f"s{i}" for i in range(6)})

    assert tokens <= 800
    assert block.count("[core]") == 6


def test_control_oracle_flags_real_overreach_and_respects_negation():
    module = _load(CONTROL_AUDIT, "audit_delivery_controls_test")

    forced = "Even here, ROLLBACK first to verify, then re-run with COMMIT."
    exempted = "No BEGIN+ROLLBACK preview is required; update the disposable fixture."
    protected = "Do not kill, restart, or interrupt the running signed build."

    assert module.audit_answer("control-test-db", forced)
    assert not module.audit_answer("control-test-db", exempted)
    assert not module.audit_answer("control-unrelated-process", protected)


def test_control_oracle_does_not_treat_css_aspect_ratio_as_model_settings():
    module = _load(CONTROL_AUDIT, "audit_delivery_controls_css_test")
    answer = "Use CSS Grid with aspect-ratio: 4/3 and object-fit: cover."
    assert not module.audit_answer("control-image-grid", answer)


def test_alias_enrichment_is_bounded_deduplicated_and_non_mutating():
    module = _load(ALIAS_RETRIEVAL, "run_evidence_alias_retrieval_test")
    source = [{"id": "r", "rule": "Canonical.", "support": [
        {"evidence": "  rough   user wording  "},
        {"evidence": "rough user wording"},
        {"evidence": "second phrase that will be capped"},
    ]}]

    enriched = module.enrich_records(source, max_chars=27)

    assert source[0]["rule"] == "Canonical."
    assert enriched[0]["retrieval_aliases"] == ["rough user wording", "second ph"]
    assert enriched[0]["rule"].endswith("rough user wording | second ph")


def test_alias_hybrid_combines_only_paired_a_t_g_answers():
    module = _load(ALIAS_HYBRID, "run_alias_hybrid_test")
    cases = [{"case_id": "one"}, {"case_id": "two"}]
    primary = {"answers": [
        {"arm": arm, "case_id": case_id, "answer": f"{arm}-{case_id}"}
        for arm in ("A", "T", "D") for case_id in ("one", "two")
    ]}
    arm_g = {"answers": [
        {"arm": "G", "case_id": case_id, "answer": f"G-{case_id}"}
        for case_id in ("one", "two")
    ]}

    combined = module.combine_answers(primary, arm_g, cases)

    assert len(combined["answers"]) == 6
    assert {row["arm"] for row in combined["answers"]} == {"A", "T", "G"}
