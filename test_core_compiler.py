from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "lib"))

import core_compiler
import adapt as adapt_cli
from memory import adapt_core


def _records():
    return [
        {"id": f"adapt-workflow-rule-{i:010x}",
         "rule": f"Apply broad durable preference number {i} across relevant engineering tasks.",
         "record_type": "standing_preference", "scope": "D--Claude"}
        for i in range(6)
    ]


def _response(records):
    return {"text": json.dumps({"rules": [
        {"text": f"Apply durable preference {i} when relevant.",
         "source_ids": [records[i]["id"]]}
        for i in range(6)
    ]}), "model": "fake", "stop_reason": "end_turn", "usage": {}}


def test_compile_core_emits_versioned_loadable_artifact(monkeypatch, tmp_path):
    records = _records()
    monkeypatch.setattr(core_compiler.adapt_sessions, "scan_batch_for_secrets_str",
                        lambda _text: True)
    out = tmp_path / "core.json"
    result = core_compiler.compile_and_write(
        records, out, lane="minimax", call=lambda *_args, **_kwargs: _response(records)
    )
    assert result["schema_version"] == adapt_core.SCHEMA_VERSION
    assert result["policy_version"] == adapt_core.POLICY_VERSION
    assert len(result["content_sha256"]) == 64
    loaded = adapt_core.load_core(out)
    assert loaded is not None and len(loaded.source_ids) == 6


def test_compile_core_accepts_legacy_unclassified_adapt_preferences(monkeypatch, tmp_path):
    records = _records()
    for record in records:
        record["record_type"] = "unclassified"
    monkeypatch.setattr(core_compiler.adapt_sessions, "scan_batch_for_secrets_str",
                        lambda _text: True)
    out = tmp_path / "core.json"
    result = core_compiler.compile_and_write(
        records, out, lane="minimax", call=lambda *_args, **_kwargs: _response(records)
    )
    assert len(result["rules"]) == 6
    assert adapt_core.load_core(out) is not None


def test_compile_core_rejects_unknown_provenance(monkeypatch, tmp_path):
    records = _records()
    bad = _response(records)
    bad["text"] = bad["text"].replace(records[0]["id"], "adapt-unknown-source")
    monkeypatch.setattr(core_compiler.adapt_sessions, "scan_batch_for_secrets_str",
                        lambda _text: True)
    try:
        core_compiler.compile_and_write(
            records, tmp_path / "core.json", lane="minimax",
            call=lambda *_args, **_kwargs: bad,
        )
    except ValueError as exc:
        assert "source" in str(exc)
    else:
        raise AssertionError("unknown source id must fail")


def test_compile_core_scanner_block_prevents_provider_call(monkeypatch, tmp_path):
    monkeypatch.setattr(core_compiler.adapt_sessions, "scan_batch_for_secrets_str",
                        lambda _text: False)
    called = []
    try:
        core_compiler.compile_and_write(
            _records(), tmp_path / "core.json", lane="minimax",
            call=lambda *_args, **_kwargs: called.append(True),
        )
    except RuntimeError as exc:
        assert "scanner" in str(exc)
    else:
        raise AssertionError("scanner-positive core input must fail")
    assert called == [] and not (tmp_path / "core.json").exists()


def test_adapt_compile_core_cli_is_an_early_non_memright_path(monkeypatch, tmp_path):
    out = tmp_path / "core.json"
    monkeypatch.setattr(sys, "argv", [
        "adapt.py", "--compile-core", str(out), "--lane", "minimax",
        "--allow-external-lane",
    ])
    monkeypatch.setattr(adapt_cli.ts, "scanner_available", lambda: True)
    monkeypatch.setattr(adapt_cli.adapt_llm, "lane_available", lambda _lane: True)
    monkeypatch.setattr(adapt_cli, "load_rules", lambda: {r["id"]: r for r in _records()})
    calls = []
    monkeypatch.setattr(
        adapt_cli.core_compiler, "compile_and_write",
        lambda records, target, **kwargs: calls.append((records, target, kwargs)) or {
            "rules": [1] * 6, "estimated_tokens": 100,
        },
    )
    monkeypatch.setattr(
        adapt_cli, "_run_memright",
        lambda _args: (_ for _ in ()).throw(AssertionError("core compile must not call MemRight")),
    )
    assert adapt_cli.main() == 0
    assert calls and calls[0][1] == out
