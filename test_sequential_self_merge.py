"""Stateful sequential preference compiler contract tests."""
from __future__ import annotations

import importlib.util
import hashlib
import json
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).parent / "eval" / "run_sequential_self_merge.py"


def _module():
    assert MODULE_PATH.is_file(), "sequential self-merge runner must exist"
    spec = importlib.util.spec_from_file_location("run_sequential_self_merge", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_group_rows_preserves_order_and_respects_budget():
    runner = _module()
    rows = [
        {"session_id": "s1", "text": "a" * 40},
        {"session_id": "s2", "text": "b" * 40},
        {"session_id": "s3", "text": "c" * 200},
    ]

    groups = runner.group_rows(rows, budget=160)

    assert [item["session_id"] for group in groups for item in group] == ["s1", "s2", "s3"]
    assert len(groups) == 2
    assert groups[1][0]["session_id"] == "s3"


def test_load_initial_taste_preserves_rules_as_baseline(tmp_path):
    runner = _module()
    taste = tmp_path / "taste.md"
    taste.write_text(
        "# Taste\n\n# workflow\n- Keep JSONL. Confidence: 0.85\n"
        "# build\n- Sign every build. Confidence: 0.80\n",
        encoding="utf-8",
    )

    records = runner.load_initial_taste(taste, workspace_scope="D--Claude")

    assert [item["rule"] for item in records] == ["Keep JSONL.", "Sign every build."]
    assert {item["status"] for item in records} == {"baseline"}
    assert {item["scope"] for item in records} == {"D--Claude"}


def test_bisect_group_preserves_order_and_balances_serialized_size():
    runner = _module()
    group = [
        {"session_id": f"s{index}", "text": chr(96 + index) * size}
        for index, size in enumerate((20, 80, 30, 70), 1)
    ]

    left, right = runner.bisect_group(group)

    assert [row["session_id"] for row in left + right] == ["s1", "s2", "s3", "s4"]
    assert left and right
    sizes = [len(json.dumps(part, ensure_ascii=False)) for part in (left, right)]
    assert max(sizes) <= min(sizes) * 2


def test_load_commandcode_groups_preserves_frozen_batch_boundaries(tmp_path):
    runner = _module()
    rows = [
        {"ordinal": 1, "session_id": "s1", "source": "codex", "text": "one"},
        {"ordinal": 2, "session_id": "s1", "source": "codex", "text": "two"},
        {"ordinal": 3, "session_id": "s2", "source": "claude", "text": "three"},
    ]
    prompt_bytes = ("\n".join(json.dumps(row) for row in rows) + "\n").encode()
    (tmp_path / "prompts.jsonl").write_bytes(prompt_bytes)
    (tmp_path / "batches").mkdir()
    batches = []
    for index, (text, count) in enumerate((("one\ntwo", 2), ("three", 1)), 1):
        data = text.encode()
        path = tmp_path / "batches" / f"batch-{index:03d}.txt"
        path.write_bytes(data)
        batches.append({
            "index": index, "path": path.relative_to(tmp_path).as_posix(),
            "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
            "prompt_count": count,
        })
    manifest = {
        "cwd": "D:\\Claude", "prompt_count": 3, "batch_count": 2,
        "prompts": {"path": "prompts.jsonl", "bytes": len(prompt_bytes),
                    "sha256": hashlib.sha256(prompt_bytes).hexdigest()},
        "batches": batches,
    }
    (tmp_path / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")

    groups, loaded = runner.load_commandcode_groups(tmp_path, workspace_scope="D--Claude")

    assert loaded["prompt_count"] == 3
    assert loaded["morph_sanitization"] == {"excluded_rows": 0, "redacted_rows": 0, "kept_rows": 3}
    assert [[row["session_id"] for row in group] for group in groups] == [["s1", "s1"], ["s2"]]
    assert all(row["scope"] == "D--Claude" for group in groups for row in group)


def test_parse_state_keeps_only_grounded_canonical_rules():
    runner = _module()
    source_rows = {
        "s1": [{"text": "I prefer JSONL for structured logs.", "scope": "D--Claude"}],
        "s2": [{"text": "One-off: edit this file now.", "scope": "D--Claude"}],
    }
    raw = json.dumps([
        {
            "record_type": "standing_preference", "category": "code-style",
            "scope": "D--Claude",
            "rule": "Use JSONL for structured logs.",
            "confidence": 0.8,
            "support": [{"session_id": "s1", "evidence": "prefer JSONL"}],
        },
        {
            "record_type": "standing_preference", "category": "made-up",
            "scope": "D--Claude",
            "rule": "Invent a category.", "confidence": 0.8,
            "support": [{"session_id": "s1", "evidence": "prefer JSONL"}],
        },
        {
            "record_type": "standing_preference", "category": "workflow",
            "scope": "D--Claude",
            "rule": "Always deploy immediately.", "confidence": 0.8,
            "support": [{"session_id": "s2", "evidence": "not present"}],
        },
    ])

    state, rejects = runner.parse_state(
        raw, source_rows=source_rows, max_rules=60, workspace_scope="D--Claude"
    )

    assert len(state) == 1
    assert state[0]["category"] == "code-style"
    assert state[0]["name"].startswith("taste-code-style-")
    assert {item["reason"] for item in rejects} == {"invalid-category", "unsupported-evidence"}


def test_parse_state_accepts_markdown_normalized_evidence_and_sixty_word_rule():
    runner = _module()
    rule = " ".join(["Keep"] + ["bounded"] * 58 + ["output"])
    raw = json.dumps([{
        "record_type": "standing_preference", "category": "workflow",
        "scope": "D--Claude", "rule": rule, "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "cwd=review_dir, timeout=600"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_texts={"s1": ["Use `cwd=review_dir`, `timeout=600` for the wrapper."]},
        max_rules=60,
    )

    assert len(rule.split()) == 60
    assert len(state) == 1
    assert rejects == []


def test_parse_state_rejects_conflicting_exact_duplicates():
    runner = _module()
    source_rows = {
        "s1": [{"text": "Always verify the real interface.", "scope": "D--Claude"}]
    }
    item = {
        "record_type": "standing_preference", "category": "verification",
        "scope": "D--Claude",
        "rule": "Always verify the real interface.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "verify the real interface"}],
    }

    state, rejects = runner.parse_state(
        json.dumps([item, {**item, "confidence": 0.6}]),
        source_rows=source_rows, max_rules=60, workspace_scope="D--Claude",
    )

    assert len(state) == 1
    assert len(rejects) == 1
    assert rejects[0]["reason"] == "exact-duplicate"


def test_parser_repairs_only_repeated_allowed_key_typo():
    runner = _module()
    raw = '[{"record_type":"standing_preference","category":"workflow","rule","rule":"Use JSONL.",' \
          '"scope":"D--Claude","confidence":0.8,' \
          '"support":[{"session_id":"s1","evidence":"Use JSONL"}]}]'

    state, rejects = runner.parse_state(
        raw, source_texts={"s1": ["Use JSONL for logs."]}, max_rules=60
    )

    assert len(state) == 1
    assert rejects == []


def test_parser_repairs_one_object_appended_after_closed_array():
    runner = _module()
    first = {
        "record_type": "standing_preference", "category": "workflow",
        "scope": "D--Claude", "rule": "Use JSONL.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Use JSONL"}],
    }
    second = {
        "record_type": "standing_preference", "category": "verification",
        "scope": "D--Claude", "rule": "Run tests.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Run tests"}],
    }
    malformed = json.dumps([first]) + "\n\n" + json.dumps(second) + "]"

    state, rejects = runner.parse_state(
        malformed,
        source_texts={"s1": ["Use JSONL. Run tests."]},
        max_rules=60,
    )

    assert [item["rule"] for item in state] == ["Use JSONL.", "Run tests."]
    assert rejects == []


def test_parser_accepts_complete_json_object_stream():
    runner = _module()
    rows = [
        {
            "record_type": "standing_preference", "category": "workflow",
            "scope": "D--Claude", "rule": "Use JSONL.", "confidence": 0.8,
            "support": [{"session_id": "s1", "evidence": "Use JSONL"}],
        },
        {
            "record_type": "standing_preference", "category": "verification",
            "scope": "D--Claude", "rule": "Run tests.", "confidence": 0.8,
            "support": [{"session_id": "s1", "evidence": "Run tests"}],
        },
    ]
    stream = "\n".join(json.dumps(item) for item in rows)

    state, rejects = runner.parse_state(
        stream,
        source_texts={"s1": ["Use JSONL. Run tests."]},
        max_rules=60,
    )

    assert [item["rule"] for item in state] == ["Use JSONL.", "Run tests."]
    assert rejects == []


def test_parser_repairs_missing_outer_brace_between_jsonl_operations():
    runner = _module()
    operation = {
        "op": "add", "record_type": "standing_preference", "category": "workflow",
        "scope": "D--Claude", "rule": "Use JSONL.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Use JSONL"}],
    }
    malformed = json.dumps(operation)[:-1] + "\n" + json.dumps({"op": "noop"})

    parsed = runner._json_array(malformed)

    assert [item["op"] for item in parsed] == ["add", "noop"]


def test_parser_repairs_only_unambiguous_trailing_noop_fragment():
    runner = _module()
    complete = json.dumps({"op": "noop"})

    parsed = runner._json_array(complete + '\n{"op":"noo')

    assert parsed == [{"op": "noop"}, {"op": "noop"}]
    with pytest.raises(json.JSONDecodeError):
        runner._json_array(complete + '\n{"op":"add","rule":"truncated')


def test_parser_normalizes_only_bare_noop_token():
    runner = _module()

    assert runner._json_array("noop") == [{"op": "noop"}]
    with pytest.raises(ValueError):
        runner._json_array("add")


def test_parser_repairs_inner_quotes_only_in_complete_jsonl_objects():
    runner = _module()
    malformed = (
        '{"op":"add","rule":"Require a "Done = yes" criterion.",'
        '"support":[]}'
    )

    assert runner._json_array(malformed)[0]["rule"] == 'Require a "Done = yes" criterion.'
    with pytest.raises(json.JSONDecodeError):
        runner._json_array('{"op":"add" "rule":"missing comma"}')
    with pytest.raises(json.JSONDecodeError):
        runner._json_array('{"op":"add","rule":"truncated')


def test_parser_ignores_prose_only_before_line_started_delta_jsonl():
    runner = _module()
    operation = {"op": "noop"}
    raw = "I will analyze the evidence first.\nThis prose is not actionable.\n\n" + json.dumps(operation)

    parsed = runner._json_array(raw)

    assert parsed == [operation]
    with pytest.raises(json.JSONDecodeError):
        runner.parse_state(
        '[{"record_type":"standing_preference","category":"workflow","oops","oops":"x"}]',
            source_texts={}, max_rules=60,
        )

    missing_support_brace = (
        '[{"record_type":"standing_preference","category":"workflow","rule":"Use JSONL.","confidence":0.8,'
        '"scope":"D--Claude","support":[{"session_id":"s1","evidence":"Use JSONL"]}]'
    )
    repaired, rejects = runner.parse_state(
        missing_support_brace,
        source_texts={"s1": ["Use JSONL for logs."]}, max_rules=60,
    )
    assert len(repaired) == 1
    assert rejects == []


def test_scope_gate_allows_explicit_standing_preference_promotion():
    runner = _module()
    raw = json.dumps([{
        "record_type": "standing_preference", "category": "code-style",
        "scope": "D--Claude", "rule": "Use JSONL for structured logs.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Across every repo, I prefer JSONL"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{
            "text": "Across every repo, I prefer JSONL for structured logs.",
            "scope": "D--Claude-coderight",
        }]},
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert state[0]["scope"] == "D--Claude"
    assert state[0]["status"] == "candidate"
    assert state[0]["scope_reason"] == "explicit-standing-scope"


def test_scope_gate_quarantines_unvalidated_or_broadened_project_scope():
    runner = _module()
    raw = json.dumps([
        {
            "record_type": "locked_decision", "category": "architecture",
            "scope": "D--Claude-ReadRight", "rule": "Use the approved ReadRight hero.",
            "confidence": 0.8,
            "support": [{"session_id": "s1", "evidence": "ReadRight hero is approved"}],
        },
        {
            "record_type": "locked_decision", "category": "architecture",
            "scope": "D--Claude", "rule": "Use the approved CodeRight shell.",
            "confidence": 0.8,
            "support": [{"session_id": "s2", "evidence": "CodeRight shell is approved"}],
        },
    ])

    state, rejects = runner.parse_state(
        raw,
        source_rows={
            "s1": [{"text": "ReadRight hero is approved.", "scope": "D--Claude"}],
            "s2": [{"text": "CodeRight shell is approved.", "scope": "D--Claude-coderight"}],
        },
        scope_catalog={"D--Claude-coderight": "coderight"},
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert [item["status"] for item in state] == ["quarantined", "quarantined"]
    assert [item["scope_reason"] for item in state] == [
        "unknown-narrow-scope", "scope-broadening",
    ]


def test_scope_gate_canonicalizes_known_slash_child_scope():
    runner = _module()
    raw = json.dumps([{
        "record_type": "locked_decision", "category": "architecture",
        "scope": "D--Claude/sellright", "rule": "Keep SellRight modular.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "SellRight stays modular"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{"text": "SellRight stays modular.", "scope": "D--Claude"}]},
        scope_catalog={"D--Claude-sellright": "sellright"},
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert state[0]["scope"] == "D--Claude-sellright"
    assert state[0]["scope_reason"] == "validated-narrow-scope"
    assert state[0]["status"] == "candidate"


def test_scope_gate_narrows_root_evidence_to_unique_named_child():
    runner = _module()
    raw = json.dumps([{
        "record_type": "operational_playbook", "category": "verification",
        "scope": "D--Claude", "rule": "HeardRight evals require three hash checks.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "HeardRight requires three hash checks"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{
            "text": "HeardRight requires three hash checks before pullback.",
            "scope": "D--Claude",
        }]},
        scope_catalog={"D--Claude-heardright": "heardright"},
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert state[0]["scope"] == "D--Claude-heardright"
    assert state[0]["scope_reason"] == "named-child-evidence"
    assert state[0]["status"] == "candidate"


def test_scope_gate_canonicalizes_bare_named_child_request():
    runner = _module()
    raw = json.dumps([{
        "record_type": "locked_decision", "category": "architecture",
        "scope": "SellRight", "rule": "SellRight keeps the provider seam.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "SellRight keeps the provider seam"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{
            "text": "SellRight keeps the provider seam.", "scope": "D--Claude",
        }]},
        scope_catalog={"D--Claude-sellright": "sellright"},
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert state[0]["scope"] == "D--Claude-sellright"
    assert state[0]["status"] == "candidate"


def test_scope_gate_quarantines_multiple_named_children_without_cross_repo_language():
    runner = _module()
    raw = json.dumps([{
        "record_type": "operational_playbook", "category": "verification",
        "scope": "D--Claude", "rule": "Compare HeardRight and SellRight release checks.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "HeardRight and SellRight release checks"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{
            "text": "Compare HeardRight and SellRight release checks.",
            "scope": "D--Claude",
        }]},
        scope_catalog={
            "D--Claude-heardright": "heardright",
            "D--Claude-sellright": "sellright",
        },
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert state[0]["scope"] == "D--Claude"
    assert state[0]["scope_reason"] == "ambiguous-named-child"
    assert state[0]["status"] == "quarantined"


def test_scope_gate_does_not_match_short_child_name_inside_larger_word():
    runner = _module()
    raw = json.dumps([{
        "record_type": "operational_playbook", "category": "tooling",
        "scope": "D--Claude", "rule": "Invoke the binary directly.",
        "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "invoke the binary directly"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{
            "text": "Always invoke the binary directly.", "scope": "D--Claude",
        }]},
        scope_catalog={"D--Claude-bin": "bin"},
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert state[0]["scope"] == "D--Claude"
    assert state[0]["status"] == "candidate"


def test_authority_gate_quarantines_literal_conflict(tmp_path):
    runner = _module()
    (tmp_path / "AGENTS.md").write_text("Never skip tests.\n", encoding="utf-8")
    manifest = runner.authority.build_manifest(tmp_path)
    raw = json.dumps([{
        "record_type": "standing_preference", "category": "verification",
        "scope": "D--Claude", "rule": "Skip tests.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "skip tests"}],
    }])

    state, rejects = runner.parse_state(
        raw,
        source_rows={"s1": [{"text": "Please skip tests.", "scope": "D--Claude"}]},
        max_rules=60,
        workspace_scope="D--Claude",
        authority_manifest=manifest,
        authority_root=tmp_path,
    )

    assert rejects == []
    assert state[0]["status"] == "quarantined"
    assert state[0]["authority_reason"] == "authority-conflict"


def test_delta_transition_adds_updates_and_deletes_grounded_rules():
    runner = _module()
    source_rows = {
        "s1": [{"text": "Always use JSONL. Replace logfmt. Stop keeping the old rule.", "scope": "D--Claude"}]
    }
    existing = [{
        "name": "taste-code-style-old", "record_type": "standing_preference",
        "authority_effect": "neutral", "authority_reason": "ok",
        "scope": "D--Claude", "scope_reason": "evidence-scope", "status": "candidate",
        "category": "code-style", "rule": "Use logfmt.", "confidence": 0.7,
        "support": [{"session_id": "s0", "evidence": "Use logfmt"}],
    }, {
        "name": "taste-workflow-delete", "record_type": "standing_preference",
        "authority_effect": "neutral", "authority_reason": "ok",
        "scope": "D--Claude", "scope_reason": "evidence-scope", "status": "candidate",
        "category": "workflow", "rule": "Keep the old rule.", "confidence": 0.6,
        "support": [{"session_id": "s0", "evidence": "Keep the old rule"}],
    }]
    operations = [
        {
            "op": "update", "target": "taste-code-style-old",
            "record_type": "standing_preference", "category": "code-style",
            "scope": "D--Claude", "rule": "Use JSONL.", "confidence": 0.9,
            "support": [{"session_id": "s1", "evidence": "Always use JSONL"}],
        },
        {
            "op": "delete", "target": "taste-workflow-delete", "reason": "contradicted",
            "support": [{"session_id": "s1", "evidence": "Stop keeping the old rule"}],
        },
        {
            "op": "add", "record_type": "standing_preference", "category": "verification",
            "scope": "D--Claude", "rule": "Replace logfmt.", "confidence": 0.8,
            "support": [{"session_id": "s1", "evidence": "Replace logfmt"}],
        },
    ]

    state, rejects = runner.apply_delta(
        "\n".join(json.dumps(item) for item in operations),
        current_state=existing,
        source_rows=source_rows,
        max_rules=60,
        workspace_scope="D--Claude",
    )

    assert rejects == []
    assert [item["rule"] for item in state] == ["Use JSONL.", "Replace logfmt."]
    assert state[0]["name"] == "taste-code-style-old"


def test_delta_transition_fails_closed_when_every_operation_is_invalid():
    runner = _module()
    raw = json.dumps({
        "op": "delete", "target": "missing", "reason": "contradicted",
        "support": [{"session_id": "s1", "evidence": "not present"}],
    })

    with pytest.raises(ValueError, match="zero valid operations"):
        runner.apply_delta(
            raw,
            current_state=[],
            source_rows={"s1": [{"text": "Different text.", "scope": "D--Claude"}]},
            max_rules=60,
            workspace_scope="D--Claude",
        )


def test_delta_transition_rejects_over_cap_add_without_aborting_batch():
    runner = _module()
    existing = [{
        "name": "taste-workflow-existing",
        "record_type": "standing_preference",
        "authority_effect": "neutral",
        "authority_reason": "ok",
        "scope": "D--Claude",
        "scope_reason": "evidence-scope",
        "status": "candidate",
        "category": "workflow",
        "rule": "Keep the incumbent.",
        "confidence": 0.8,
        "support": [{"session_id": "s0", "evidence": "Keep the incumbent"}],
    }]
    raw = json.dumps({
        "op": "add", "record_type": "standing_preference", "category": "workflow",
        "scope": "D--Claude", "rule": "Add another rule.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Add another rule"}],
    })

    state, rejects = runner.apply_delta(
        raw,
        current_state=existing,
        source_rows={"s1": [{"text": "Always add another rule.", "scope": "D--Claude"}]},
        max_rules=1,
        workspace_scope="D--Claude",
    )

    assert state == existing
    assert rejects == [{"index": 0, "reason": "rule-cap"}]


def test_compiler_carries_state_and_resume_replays_cached_calls(tmp_path):
    runner = _module()
    groups = [
        [{"session_id": "s1", "text": "I prefer JSONL for logs."}],
        [{"session_id": "s2", "text": "Always verify the real interface."}],
    ]
    seen_existing = []

    def fake_call(_system, user, _max_tokens):
        payload = json.loads(user)
        seen_existing.append(len(payload["existing_rules"]))
        rules = list(payload["existing_rules"])
        if not rules:
            rules.append({
                "record_type": "standing_preference", "category": "code-style",
                "scope": "D--Claude",
                "rule": "Use JSONL for logs.",
                "confidence": 0.8,
                "support": [{"session_id": "s1", "evidence": "prefer JSONL"}],
            })
        else:
            rules.append({
                "record_type": "standing_preference", "category": "verification",
                "scope": "D--Claude",
                "rule": "Always verify the real interface.",
                "confidence": 0.8,
                "support": [{"session_id": "s2", "evidence": "verify the real interface"}],
            })
        return json.dumps(rules)

    first = runner.run_compiler(
        groups=groups, output_dir=tmp_path, call_fn=fake_call,
        scanner=lambda _text: True, max_rules=60, max_provider_calls=2,
        max_input_chars=100_000, max_tokens=4096, resume=False,
    )
    assert seen_existing == [0, 1]
    assert len(first["rules"]) == 2
    assert first["ledger"]["provider_calls"] == 2

    second = runner.run_compiler(
        groups=groups, output_dir=tmp_path,
        call_fn=lambda *_args: (_ for _ in ()).throw(AssertionError("provider called")),
        scanner=lambda _text: True, max_rules=60, max_provider_calls=2,
        max_input_chars=100_000, max_tokens=4096, resume=True,
    )
    assert second["ledger"]["cache_hits"] == 2
    assert second["rules"] == first["rules"]


def test_delta_compiler_starts_from_seeded_state(tmp_path):
    runner = _module()
    baseline = [{
        "name": "taste-workflow-seed",
        "record_type": "standing_preference",
        "authority_effect": "neutral",
        "authority_reason": "seeded-baseline",
        "scope": "D--Claude",
        "scope_reason": "seeded-baseline",
        "status": "baseline",
        "category": "workflow",
        "rule": "Keep the baseline.",
        "confidence": 0.8,
        "support": [],
    }]

    result = runner.run_compiler(
        groups=[[{"session_id": "s1", "text": "Nothing durable here."}]],
        output_dir=tmp_path,
        call_fn=lambda _system, user, _tokens: (
            json.dumps({"op": "noop"})
            if len(json.loads(user)["existing_rules"]) == 1
            else (_ for _ in ()).throw(AssertionError("baseline missing"))
        ),
        scanner=lambda _text: True,
        max_rules=60,
        max_provider_calls=1,
        max_input_chars=100_000,
        max_tokens=4096,
        resume=False,
        mode="delta",
        initial_state=baseline,
    )

    assert result["rules"] == baseline


def test_compiler_marks_parse_failure_retryable_on_resume(tmp_path):
    runner = _module()
    groups = [[{"session_id": "s1", "scope": "D--Claude", "text": "Use JSONL."}]]

    with pytest.raises(ValueError, match="provider error"):
        runner.run_compiler(
            groups=groups,
            output_dir=tmp_path,
            call_fn=lambda *_args: '{"error":"input truncated"}',
            scanner=lambda _text: True,
            max_rules=60,
            max_provider_calls=1,
            max_input_chars=100_000,
            max_tokens=4096,
            resume=False,
        )

    failed = json.loads((tmp_path / "calls" / "call-001.json").read_text(encoding="utf-8"))
    assert failed["status"] == "parse_error"
    assert "provider error" in failed["parse_error"]

    valid = json.dumps([{
        "record_type": "standing_preference", "category": "code-style",
        "scope": "D--Claude", "rule": "Use JSONL.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Use JSONL"}],
    }])
    with pytest.raises(RuntimeError, match="provider-call ceiling"):
        runner.run_compiler(
            groups=groups,
            output_dir=tmp_path,
            call_fn=lambda *_args: (_ for _ in ()).throw(AssertionError("provider called")),
            scanner=lambda _text: True,
            max_rules=60,
            max_provider_calls=1,
            max_input_chars=100_000,
            max_tokens=4096,
            resume=True,
        )

    resumed = runner.run_compiler(
        groups=groups,
        output_dir=tmp_path,
        call_fn=lambda *_args: valid,
        scanner=lambda _text: True,
        max_rules=60,
        max_provider_calls=2,
        max_input_chars=100_000,
        max_tokens=4096,
        resume=True,
    )

    assert [item["rule"] for item in resumed["rules"]] == ["Use JSONL."]
    assert resumed["ledger"]["cumulative_provider_calls"] == 2


def test_compiler_treats_provider_abort_as_retryable(tmp_path):
    runner = _module()
    groups = [[{"session_id": "s1", "scope": "D--Claude", "text": "Use JSONL."}]]

    with pytest.raises(RuntimeError, match="provider stopped"):
        runner.run_compiler(
            groups=groups,
            output_dir=tmp_path,
            call_fn=lambda *_args: {
                "text": '{"record_type":"standing_preference"',
                "stop_reason": "abort",
            },
            scanner=lambda _text: True,
            max_rules=60,
            max_provider_calls=1,
            max_input_chars=100_000,
            max_tokens=4096,
            resume=False,
        )

    failed = json.loads((tmp_path / "calls" / "call-001.json").read_text(encoding="utf-8"))
    assert failed["status"] == "provider_abort"


def test_compiler_can_reparse_cached_error_without_provider_call(tmp_path):
    runner = _module()
    groups = [[{"session_id": "s1", "scope": "D--Claude", "text": "Use JSONL."}]]
    valid = json.dumps([{
        "record_type": "standing_preference", "category": "code-style",
        "scope": "D--Claude", "rule": "Use JSONL.", "confidence": 0.8,
        "support": [{"session_id": "s1", "evidence": "Use JSONL"}],
    }])
    runner.run_compiler(
        groups=groups, output_dir=tmp_path, call_fn=lambda *_args: valid,
        scanner=lambda _text: True, max_rules=60, max_provider_calls=1,
        max_input_chars=100_000, max_tokens=4096, resume=False,
    )
    call_path = tmp_path / "calls" / "call-001.json"
    cached = json.loads(call_path.read_text(encoding="utf-8"))
    cached["status"] = "parse_error"
    call_path.write_text(json.dumps(cached), encoding="utf-8")

    replayed = runner.run_compiler(
        groups=groups, output_dir=tmp_path,
        call_fn=lambda *_args: (_ for _ in ()).throw(AssertionError("provider called")),
        scanner=lambda _text: True, max_rules=60, max_provider_calls=1,
        max_input_chars=100_000, max_tokens=4096, resume=True,
        reparse_errors=True,
    )

    assert replayed["ledger"]["reparse_hits"] == 1
    assert replayed["rules"][0]["rule"] == "Use JSONL."
