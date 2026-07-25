from __future__ import annotations

import copy
import io
import importlib.util
import itertools
import json
import subprocess
import sys
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FIXTURE = json.loads((HERE / "fixtures" / "planner-conformance-v1.json").read_text(encoding="utf-8"))
SPEC = importlib.util.spec_from_file_location(
    "recall_planner_conformance", ROOT / "tools" / "hooks" / "recall_planner.py"
)
rp = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = rp
SPEC.loader.exec_module(rp)


def _cases():
    dimensions = FIXTURE["dimensions"]
    return list(itertools.product(
        dimensions["scopeForms"], dimensions["graphStates"], dimensions["outcomes"]
    ))


def _payload(scope_form, graph_state, outcome):
    template = FIXTURE["packetTemplate"]
    blocks = copy.deepcopy(template["blocks"])
    for block in blocks:
        if block.get("resolver") == "__OVERSIZED_RESOLVER__":
            block["resolver"] = "read " + "x" * 12_000
    provenance = {
        field: graph_state[field]
        for field in ("baseCommit", "overlayDigest", "freshnessClass")
        if graph_state.get(field)
    }
    for block in blocks:
        block.update(provenance)
    packet = {
        "traceId": template["traceId"],
        "budget": copy.deepcopy(template["budget"]),
        "blocks": blocks,
        "freshness": {
            "stale": graph_state["stale"],
            "graphState": graph_state["id"],
            "upstreamScopeForm": scope_form["id"],
            "upstreamScopeInput": scope_form["input"],
            "canonicalScope": scope_form["canonicalScope"],
            **{
                field: graph_state[field]
                for field in ("baseCommit", "overlayDigest")
                if graph_state.get(field)
            },
        },
    }
    return {
        "traceId": template["traceId"],
        "providerStatus": template["providerStatus"],
        "sourceGeneration": template["sourceGeneration"],
        "fallbackMode": outcome["fallbackMode"],
        "degradationReason": outcome["degradationReason"],
        "packet": packet,
        "receipts": [
            {
                "id": block["id"],
                "decision": "admitted",
                "deliveryStage": "planned",
                "deliveryClass": block["deliveryClass"],
                "selectedTokens": block["selectedTokens"],
                "renderedTokens": 0,
                "deliveredChars": 0,
                "dropReason": block["dropReason"],
                **provenance,
            }
            for block in blocks
        ],
    }


def test_python_planner_conforms_to_shared_matrix():
    expected = FIXTURE["expected"]
    cases = _cases()
    assert len(cases) == expected["caseCount"]
    for scope_form, graph_state, outcome in cases:
        payload = _payload(scope_form, graph_state, outcome)
        disposition = rp._delivery_disposition(payload)
        assert disposition == {
            "deliver": outcome["expectedDeliver"],
            "fallbackMode": outcome["fallbackMode"],
            "degradationReason": outcome["degradationReason"],
        }
        assert payload["packet"]["budget"] == {"maxTokens": 4096, "admittedTokens": 40}
        assert payload["packet"]["freshness"]["canonicalScope"] == "D--Claude"
        if graph_state["id"] == "dirty_overlay":
            provenance = expected["dirtyProvenance"]
            assert all(
                block["baseCommit"] == provenance["baseCommit"]
                and block["overlayDigest"] == provenance["overlayDigest"]
                and block["freshnessClass"] == provenance["freshnessClass"]
                for block in payload["packet"]["blocks"]
            )
            assert all(
                receipt["baseCommit"] == provenance["baseCommit"]
                and receipt["overlayDigest"] == provenance["overlayDigest"]
                and receipt["freshnessClass"] == provenance["freshnessClass"]
                for receipt in payload["receipts"]
            )
        if not outcome["expectedDeliver"]:
            assert all(block["deliveryStage"] == "planned" for block in payload["packet"]["blocks"])
            continue

        memory_seals: list[tuple[str, str]] = []
        skill_seals: list[tuple[str, str, str]] = []
        rendered = rp._format_packet_block(
            payload,
            verify_memory=lambda memory_id, sha: memory_seals.append((memory_id, sha)) or True,
            verify_skill=lambda name, description, sha: skill_seals.append(
                (name, description, sha)
            ) or True,
        )
        blocks = payload["packet"]["blocks"]
        assert [block["deliveryClass"] for block in blocks] == expected["deliveryClasses"]
        assert [block["dropReason"] for block in blocks] == expected["dropReasons"]
        assert memory_seals == [(
            expected["memorySeal"]["id"], expected["memorySeal"]["sourceHash"]
        )]
        assert skill_seals == [(
            expected["skillSeal"]["name"], expected["skillSeal"]["description"],
            expected["skillSeal"]["bodyHash"],
        )]
        assert rp._DELIVERY_STATS["selected_tokens"] == expected["selectedTokens"]
        assert rp._DELIVERY_STATS["planner_selected_tokens"] == expected["plannerSelectedTokens"]
        for provider, accounting in expected["providerAccounting"].items():
            actual = rp._DELIVERY_STATS["provider_accounting"][provider]
            assert actual["selected_tokens"] == accounting["selected_tokens"]
            assert actual["planner_selected_tokens"] == accounting["planner_selected_tokens"]
        assert all(receipt["deliveryStage"] == "finalized" for receipt in payload["receipts"])
        assert [receipt["deliveryClass"] for receipt in payload["receipts"]] == expected["deliveryClasses"]
        assert len(rendered) <= 10_000
        assert rp._DELIVERY_STATS["delivery_state"] == "char_cap_truncated"
        assert rp._DELIVERY_STATS["char_cap_dropped_blocks"] == 1
        assert rp._DELIVERY_STATS["block_reconciles"] is True
        assert rp._DELIVERY_STATS["token_reconciles"] is True
        assert "SECRET FIXTURE CANDIDATE TEXT" not in rendered


def test_python_public_planner_drives_the_shared_matrix(monkeypatch, capsys):
    expected = FIXTURE["expected"]
    monkeypatch.setenv("RIGHTCONTEXT_MODE", "on")
    monkeypatch.setenv("RIGHTCONTEXT_COHORTS", "on")
    monkeypatch.setenv("RIGHTCONTEXT_POLICY_ENV_OVERRIDE", "1")
    monkeypatch.setenv("RIGHTCONTEXT_POLICY_TEST", "1")
    monkeypatch.setenv("MEMRIGHT_CLIENT", "claude")
    monkeypatch.setattr(rp, "_heartbeat", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(rp, "_delivery_record", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(rp, "_fallback_event", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(
        rp.recall_legacy,
        "main",
        lambda: print(json.dumps({
            "hookSpecificOutput": {"additionalContext": "CONFORMANCE_LEGACY"}
        })) or 0,
    )

    for scope_form, graph_state, outcome in _cases():
        payload = _payload(scope_form, graph_state, outcome)
        invoke_calls: list[dict] = []
        memory_seals: list[tuple[str, str]] = []
        skill_seals: list[tuple[str, str, str]] = []

        def fake_invoke(**kwargs):
            invoke_calls.append(kwargs)
            return payload

        class FakeResolver:
            def __init__(self, **_kwargs):
                pass

            def verify_catalog(self, name, description, sha):
                skill_seals.append((name, description, sha))
                return True

        monkeypatch.setattr(rp, "_invoke_federation", fake_invoke)
        monkeypatch.setattr(rp, "SkillResolver", FakeResolver)
        monkeypatch.setattr(
            rp,
            "_verify_memory_row",
            lambda memory_id, sha, **_kwargs: memory_seals.append((memory_id, sha)) or True,
        )
        monkeypatch.setattr(
            sys,
            "stdin",
            io.StringIO(json.dumps({
                "prompt": "fixture prompt",
                "session_id": "fixture-session",
                "cwd": str(ROOT),
            })),
        )
        assert rp.main() == 0
        emitted = capsys.readouterr().out.strip()
        assert invoke_calls[0]["max_tokens"] == 4096
        assert Path(invoke_calls[0]["repo"]).resolve() == ROOT.resolve()
        assert payload["packet"]["freshness"]["upstreamScopeInput"] == scope_form["input"]
        assert payload["packet"]["freshness"]["graphState"] == graph_state["id"]

        if outcome["expectedDeliver"]:
            output = json.loads(emitted.splitlines()[-1])
            additional = output["hookSpecificOutput"]["additionalContext"]
            assert "CONFORMANCE_LEGACY" not in additional
            assert len(additional) <= 10_000
            assert "SECRET FIXTURE CANDIDATE TEXT" not in additional
            assert [block["deliveryClass"] for block in payload["packet"]["blocks"]] == expected["deliveryClasses"]
            assert [receipt["deliveryStage"] for receipt in payload["receipts"]] == [
                "finalized"
            ] * len(payload["receipts"])
            assert memory_seals == [(
                expected["memorySeal"]["id"], expected["memorySeal"]["sourceHash"]
            )]
            assert skill_seals == [(
                expected["skillSeal"]["name"], expected["skillSeal"]["description"],
                expected["skillSeal"]["bodyHash"],
            )]
            assert rp._DELIVERY_STATS["selected_tokens"] == expected["selectedTokens"]
            assert rp._DELIVERY_STATS["planner_selected_tokens"] == expected["plannerSelectedTokens"]
            assert rp._DELIVERY_STATS["delivery_state"] == "char_cap_truncated"
            assert rp._DELIVERY_STATS["char_cap_dropped_blocks"] == 1
            assert rp._DELIVERY_STATS["block_reconciles"] is True
            assert rp._DELIVERY_STATS["token_reconciles"] is True
        else:
            assert emitted == ""
            assert memory_seals == [] and skill_seals == []
            assert all(block["deliveryStage"] == "planned" for block in payload["packet"]["blocks"])
            assert all(receipt["deliveryStage"] == "planned" for receipt in payload["receipts"])


def test_codex_twin_runs_in_the_same_conformance_lane():
    twin = ROOT / "tools" / "codex-brief-plugin" / "test-planner-conformance.test.mjs"
    result = subprocess.run(
        ["node", "--test", str(twin)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
