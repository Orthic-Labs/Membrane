"""Phase 2 sealed three-arm pilot tests."""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest

MORPH_DIR = Path(__file__).resolve().parent
EVAL_DIR = MORPH_DIR / "eval"
sys.path.insert(0, str(MORPH_DIR))
sys.path.insert(0, str(EVAL_DIR))

import morph_sessions
import freeze_pilot
import run_phase2_pilot as phase2


def _typed_observation(record: dict, prompt: int) -> dict:
    return {
        "category": "tooling",
        "observation": "Preserve exact evidence for durable workflow rules.",
        "evidence": " ".join(record["text"].split()[:4]),
        "prompt": prompt,
        "record_type": "agent_preference",
        "durability": "cross_task_explicit",
        "subject": "agent_behavior",
    }


def _session(root: Path, tool: str, sid: str, text: str, mtime: float):
    path = root / f"{tool}-{sid}.jsonl"
    if tool == "claude-code":
        rows = [{
            "type": "user",
            "userType": "external",
            "sessionId": sid,
            "cwd": "D:/Claude",
            "message": {"content": text},
        }]
        parser = morph_sessions.parse_claude_session
    else:
        rows = [
            {"type": "session_meta", "payload": {
                "session_id": sid, "cwd": "D:/Claude"
            }},
            {"type": "response_item", "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": text}],
            }},
        ]
        parser = morph_sessions.parse_codex_session
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    os.utime(path, (mtime, mtime))
    parsed = parser(path)
    assert parsed is not None
    return parsed


def _pilot(tmp_path: Path) -> Path:
    workspace = tmp_path / "workspace"
    (workspace / ".claude" / "rules").mkdir(parents=True)
    (workspace / "AGENTS.md").write_text("Never bypass review.\n", encoding="utf-8")
    (workspace / "CLAUDE.md").write_text("Always preserve user changes.\n", encoding="utf-8")
    (workspace / ".claude" / "rules" / "ops.md").write_text(
        "Never edit production directly.\n", encoding="utf-8"
    )
    sessions_dir = tmp_path / "sessions"
    sessions_dir.mkdir()
    sessions = [
        _session(
            sessions_dir,
            "claude-code",
            "c1",
            "always use JSONL for structured logs in shared tooling",
            10,
        ),
        _session(
            sessions_dir,
            "codex",
            "x1",
            "never mark a batch learned after a partial provider response",
            20,
        ),
    ]
    rules = tmp_path / "rules.json"
    rules.write_text("{}", encoding="utf-8")
    out = tmp_path / "pilot"
    freeze_pilot.freeze_pilot(
        sessions,
        out,
        target=2,
        workspace_root=workspace,
        artifact_roots=[rules],
        max_raw_bytes=1_000_000,
    )
    return out / "manifest.json"


def test_load_pilot_consumes_frozen_batches_and_incumbents(tmp_path):
    pilot = phase2.load_pilot(_pilot(tmp_path))

    assert pilot.manifest["selection"]["selected_sessions"] == 2
    assert len(pilot.batches) == pilot.manifest["extraction"]["batch_count"]
    assert sum(len(batch.records) for batch in pilot.batches) == 2
    assert pilot.existing_rules == ()
    assert pilot.batches[0].rows[0]["session_id"] in {"c1", "x1"}


def test_preflight_enforces_provider_call_and_input_ceilings(tmp_path):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    report = phase2.build_preflight(
        pilot, max_provider_calls=10, max_input_chars=1_000_000
    )

    alarm_batches = sum(
        bool(phase2.morph_llm.extract_deterministic([
            (row["tool"], row["scope"], row["text"])
            for row in batch.records
        ]))
        for batch in pilot.batches
    )
    assert report["planned_provider_calls"] == len(pilot.batches) + alarm_batches + 2
    assert report["recall_alarm_batches"] == alarm_batches
    assert report["shared_extraction"] is True
    assert report["within_ceilings"] is True

    with pytest.raises(phase2.PilotError, match="provider-call ceiling"):
        phase2.build_preflight(
            pilot, max_provider_calls=1, max_input_chars=1_000_000
        )
    with pytest.raises(phase2.PilotError, match="input-character ceiling"):
        phase2.build_preflight(
            pilot, max_provider_calls=10, max_input_chars=1
        )


def test_ground_observations_rejects_nonverbatim_evidence(tmp_path):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    batch = pilot.batches[0]
    text = batch.records[0]["text"]
    good_evidence = " ".join(text.split()[:4])
    actions = [
        {"category": "tooling", "observation": "Use JSONL for structured logs.",
         "evidence": good_evidence, "prompt": 1, "source": "model",
         "record_type": "agent_preference", "durability": "cross_task_explicit",
         "subject": "agent_behavior"},
        {"category": "workflow", "observation": "Invented preference.",
         "evidence": "this phrase never appeared", "prompt": 1, "source": "model",
         "record_type": "agent_preference", "durability": "cross_task_explicit",
         "subject": "agent_behavior"},
    ]

    kept, rejected = phase2.ground_observations(actions, batch)

    assert len(kept) == 1
    assert rejected == [{"reason": "evidence-not-verbatim", "prompt": 1}]
    assert kept[0]["source_session_id"] == batch.rows[0]["session_id"]
    assert kept[0]["observation_id"].startswith("obs-")


def test_ground_observations_requires_typed_cross_task_agent_preference(tmp_path):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    batch = pilot.batches[0]
    text = batch.records[0]["text"]
    evidence = " ".join(text.split()[:4])

    kept, rejected = phase2.ground_observations([{
        "category": "tooling",
        "observation": "Always preserve exact transcript evidence.",
        "evidence": evidence,
        "prompt": 1,
    }], batch)

    assert kept == []
    assert rejected == [{"reason": "missing-preference-classification", "prompt": 1}]


def test_ground_observations_rejects_typed_lane_instruction():
    batch = phase2.FrozenBatch(
        index=1,
        payload="[]",
        records=({
            "id": 1,
            "tool": "codex",
            "scope": "D--Claude",
            "text": "Lane A: NEVER touch other crates; only edit src/a.rs.",
        },),
        rows=({
            "tool": "codex",
            "scope": "D--Claude",
            "session_id": "session-a",
            "turn_ordinal": 1,
        },),
        payload_sha256="fixture",
    )
    action = {
        "category": "workflow",
        "observation": "Never touch other crates while working in Lane A.",
        "evidence": "NEVER touch other crates",
        "prompt": 1,
        "record_type": "agent_preference",
        "durability": "cross_task_explicit",
        "subject": "agent_behavior",
    }

    kept, rejected = phase2.ground_observations([action], batch)

    assert kept == []
    assert rejected == [{"reason": "task-bound-instruction", "prompt": 1}]


def test_arm_quarantines_single_session_even_with_explicit_language(tmp_path):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    observations = [{
        "observation_id": "obs-one",
        "category": "tooling",
        "observation": "Always preserve exact transcript evidence.",
        "evidence": "always preserve exact transcript evidence",
        "scope": "D--Claude",
        "source_session_id": "session-a",
    }]
    actions = [{
        "action": "add",
        "name": "morph-tooling-preserve-transcript-evidence",
        "category": "tooling",
        "rule": "Always preserve exact transcript evidence before changing shared workflow rules.",
        "confidence": 0.8,
        "observations": 1,
        "supporting_observation_ids": ["obs-one"],
        "why": "explicit standing directive",
    }]

    arm = phase2._evaluate_arm(actions, observations, pilot)

    assert arm["admitted_count"] == 0
    assert arm["reason_counts"] == {"insufficient-independent-support": 1}


def test_execute_reuses_shared_extraction_and_cached_calls(tmp_path, monkeypatch):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets", lambda batch: True
    )
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets_str", lambda payload: True
    )
    calls = []

    def fake(system: str, user: str, max_tokens: int) -> str:
        calls.append((system, user, max_tokens))
        if system == phase2.morph_llm.EXTRACT_SYSTEM:
            records = json.loads(user)
            return json.dumps([
                _typed_observation(record, prompt)
                for prompt, record in enumerate(records, 1)
            ])
        observations = json.loads(user)["new_observations"]
        support = [item["observation_id"] for item in observations]
        if system == phase2.CURRENT_MORPH_SYSTEM:
            return json.dumps([{
                "action": "add",
                "name": "ignored",
                "category": "tooling",
                "rule": "Always preserve exact transcript evidence before changing shared workflow rules.",
                "confidence": 0.8,
                "observations": 1,
                "supporting_observation_ids": support,
                "why": "explicit standing directive",
            }])
        assert system == phase2.FULL_SET_SYSTEM
        return json.dumps([{
            "category": "tooling",
            "rule": "Always preserve exact transcript evidence before changing shared workflow rules.",
            "confidence": 0.8,
            "supporting_observation_ids": support,
            "why": "explicit standing directive",
        }])

    out = tmp_path / "phase2"
    first = phase2.execute_pilot(
        pilot,
        out,
        call_fn=fake,
        lane="minimax",
        max_provider_calls=10,
        max_input_chars=1_000_000,
    )
    first_call_count = len(calls)
    checkpoint = out / "batches" / f"batch-{pilot.batches[0].index:03d}.json"
    assert checkpoint.is_file()
    saved_batch = json.loads(checkpoint.read_text(encoding="utf-8"))
    assert saved_batch["grounded_observations"]
    assert saved_batch["provider_evidence"]

    def should_not_reparse(*_args, **_kwargs):
        raise AssertionError("completed batch checkpoint should bypass extraction parsing")

    monkeypatch.setattr(
        phase2.morph_llm, "extract_observations", should_not_reparse
    )
    second = phase2.execute_pilot(
        pilot,
        out,
        call_fn=fake,
        lane="minimax",
        max_provider_calls=10,
        max_input_chars=1_000_000,
    )

    assert first == second
    assert first_call_count == len(pilot.batches) + 2
    assert len(calls) == first_call_count
    assert first["common_extraction"]["provider_calls"] == len(pilot.batches)
    assert first["common_extraction"]["recall_provider_calls"] == 0
    assert first["common_extraction"]["raw_observation_count"] >= first["common_extraction"]["observation_count"]
    assert first["temperature"] == phase2.TEMPERATURE
    assert (out / "observations.json").is_file()
    assert first["arms"]["retrieval_only"]["candidate_count"] == 0
    assert first["arms"]["current_morph"]["admitted_count"] == 1
    assert first["arms"]["bounded_full_set"]["admitted_count"] == 1

    saved_batch["raw_observation_count"] += 1
    checkpoint.write_text(json.dumps(saved_batch), encoding="utf-8")
    with pytest.raises(phase2.PilotError, match="checkpoint content hash mismatch"):
        phase2._load_batch_checkpoint(out, pilot.batches[0], "minimax")


def test_full_set_parser_refuses_output_above_cap():
    raw = json.dumps([
        {"category": "tooling", "rule": "Always preserve exact evidence before changing shared workflow rules.",
         "confidence": 0.8, "supporting_observation_ids": ["obs-1"], "why": "x"},
        {"category": "workflow", "rule": "Always verify the complete result before reporting a task done.",
         "confidence": 0.8, "supporting_observation_ids": ["obs-2"], "why": "x"},
    ])

    with pytest.raises(phase2.PilotError, match="rule cap"):
        phase2.parse_full_set(raw, rule_cap=1)


def test_failed_provider_call_is_cached_and_retried_at_most_once(tmp_path, monkeypatch):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets", lambda batch: True
    )
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets_str", lambda payload: True
    )
    calls = 0

    def flaky(system: str, user: str, max_tokens: int) -> str:
        nonlocal calls
        calls += 1
        if calls == 1:
            raise ConnectionResetError("provider reset")
        if system == phase2.morph_llm.EXTRACT_SYSTEM:
            records = json.loads(user)
            return json.dumps([
                _typed_observation(record, prompt)
                for prompt, record in enumerate(records, 1)
            ])
        observations = json.loads(user)["new_observations"]
        common = {
            "category": "tooling",
            "rule": "Always preserve exact transcript evidence before changing shared workflow rules.",
            "confidence": 0.8,
            "supporting_observation_ids": [
                observation["observation_id"] for observation in observations
            ],
            "why": "explicit standing directive",
        }
        return json.dumps([
            {"action": "add", "name": "ignored", "observations": 1, **common}
            if system == phase2.CURRENT_MORPH_SYSTEM else common
        ])

    out = tmp_path / "retry"
    with pytest.raises(phase2.PilotError, match="provider failed"):
        phase2.execute_pilot(
            pilot, out, call_fn=flaky, max_provider_calls=10,
            max_input_chars=1_000_000,
        )
    assert calls == 1

    with pytest.raises(phase2.PilotError, match="requires --retry-failures"):
        phase2.execute_pilot(
            pilot, out, call_fn=flaky, max_provider_calls=10,
            max_input_chars=1_000_000,
        )
    assert calls == 1

    result = phase2.execute_pilot(
        pilot, out, call_fn=flaky, retry_failures=True,
        max_provider_calls=10, max_input_chars=1_000_000,
    )
    assert result["arms"]["current_morph"]["admitted_count"] == 1
    cached = json.loads((out / "calls" / "extract-001.json").read_text())
    assert cached["attempts"] == 2


def test_provider_stop_reason_truncation_is_typed_and_cached(tmp_path, monkeypatch):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets", lambda batch: True
    )

    def truncated(system: str, user: str, max_tokens: int) -> dict:
        return {
            "text": '[{"category":"tooling"',
            "model": "MiniMax-M3",
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 10, "output_tokens": max_tokens},
        }

    out = tmp_path / "truncated"
    with pytest.raises(phase2.PilotError, match="provider truncated"):
        phase2.execute_pilot(
            pilot, out, call_fn=truncated, max_provider_calls=10,
            max_input_chars=1_000_000,
        )
    cached = json.loads((out / "calls" / "extract-001.json").read_text())
    assert cached["status"] == "truncated"
    assert cached["stop_reason"] == "max_tokens"
    assert cached["usage"]["output_tokens"] == phase2.EXTRACT_MAX_TOKENS


def test_valid_empty_with_deterministic_signal_gets_one_model_recall_pass(
    tmp_path, monkeypatch
):
    pilot = phase2.load_pilot(_pilot(tmp_path))
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets", lambda batch: True
    )
    monkeypatch.setattr(
        phase2.morph_sessions, "scan_batch_for_secrets_str", lambda payload: True
    )
    extract_calls = 0

    def fake(system: str, user: str, max_tokens: int) -> str:
        nonlocal extract_calls
        if system == phase2.morph_llm.EXTRACT_SYSTEM:
            extract_calls += 1
            if extract_calls == 1:
                return "[]"
            records = json.loads(user)
            return json.dumps([
                _typed_observation(record, prompt)
                for prompt, record in enumerate(records, 1)
            ])
        observations = json.loads(user)["new_observations"]
        common = {
            "category": "tooling",
            "rule": "Always preserve exact transcript evidence before changing shared workflow rules.",
            "confidence": 0.8,
            "supporting_observation_ids": [
                observation["observation_id"] for observation in observations
            ],
            "why": "explicit standing directive",
        }
        return json.dumps([
            {"action": "add", "name": "ignored", "observations": 1, **common}
            if system == phase2.CURRENT_MORPH_SYSTEM else common
        ])

    result = phase2.execute_pilot(
        pilot, tmp_path / "recall", call_fn=fake,
        max_provider_calls=10, max_input_chars=1_000_000,
    )

    assert extract_calls == 2
    assert result["common_extraction"]["recall_provider_calls"] == 1
    assert result["common_extraction"]["observation_count"] == len(
        pilot.batches[0].records
    )
