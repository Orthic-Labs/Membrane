import json
from types import SimpleNamespace

import pytest

from federation.providers import cortex
from federation.providers.cortex import candidate_cap


def test_cli_resolution_prefers_cortex_and_retains_legacy_fallback(monkeypatch, tmp_path):
    cortex_cli = tmp_path / "cortex" / "cortex.mjs"
    legacy_cli = tmp_path / "cortex" / "cortex.mjs"
    cortex_cli.parent.mkdir()
    legacy_cli.parent.mkdir()
    cortex_cli.touch()
    legacy_cli.touch()
    monkeypatch.delenv("CORTEX_CLI", raising=False)
    monkeypatch.setattr(cortex, "CORTEX_CLI_DEFAULT", cortex_cli)
    monkeypatch.setattr(cortex, "CORTEX_SKILLS_CLI_DEFAULT", legacy_cli)

    assert cortex._resolve_cortex_cli() == str(cortex_cli)
    cortex_cli.unlink()
    assert cortex._resolve_cortex_cli() == str(legacy_cli)


def test_repo_code_candidate_cap_is_independent_of_large_context_budget():
    assert candidate_cap(2048, None) == 64
    assert candidate_cap(4096, None) == 64


def test_repo_code_candidate_cap_is_bounded_and_invalid_override_is_safe():
    assert candidate_cap(4096, "32") == 32
    assert candidate_cap(4096, "999") == 256
    assert candidate_cap(4096, "invalid") == 64


def test_observability_splits_subprocess_and_repo_scan_without_changing_produce(monkeypatch, tmp_path):
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    payload = {
        "schemaVersion": 1,
        "freshness": {"stale": False},
        "_membrane": {
            "stageElapsedMs": {
                "repo_code_scan": 12.5,
                "task": "must-not-propagate",
            }
        },
        "candidates": [
            {
                "id": "node-1",
                "sourceRef": "src/app.py:1-2",
                "sourceHash": "sha256:" + "0" * 64,
                "qualifiedName": "app.main",
            }
        ],
    }
    monotonic_values = iter((100.0, 100.04, 200.0, 200.04))
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        cortex,
        "_read_manifest",
        lambda _root: {"generationId": "generation-1", "freshness": {"stale": False}},
    )
    monkeypatch.setattr(cortex.time, "monotonic", lambda: next(monotonic_values))
    monkeypatch.setattr(
        cortex.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(payload), stderr=""
        ),
    )

    candidates, generation, warnings, observability = cortex.produce_with_observability(
        tmp_path, "secret task", 4096
    )
    legacy_result = cortex.produce(tmp_path, "secret task", 4096)

    assert generation == "generation-1"
    assert warnings == []
    assert candidates[0]["id"] == "cortex:node-1"
    assert observability == {
        "stageElapsedMs": {
            "cortex_node_spawn": 27.5,
            "repo_code_scan": 12.5,
        }
    }
    assert legacy_result[:2] == (candidates, generation)
    assert legacy_result[2] == []
    assert len(legacy_result) == 3
    assert "secret task" not in json.dumps(observability)


def test_observability_preserves_positive_submillisecond_stage_timing(monkeypatch, tmp_path):
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    payload = {
        "schemaVersion": 1,
        "_membrane": {"stageElapsedMs": {"repo_code_scan": 0.0002}},
        "candidates": [],
    }
    monotonic_values = iter((100.0, 100.0000004))
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        cortex,
        "_read_manifest",
        lambda _root: {"generationId": "generation-1", "freshness": {"stale": False}},
    )
    monkeypatch.setattr(cortex.time, "monotonic", lambda: next(monotonic_values))
    monkeypatch.setattr(
        cortex.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(payload), stderr=""
        ),
    )

    *_result, observability = cortex.produce_with_observability(
        tmp_path, "task", 4096
    )

    assert observability["stageElapsedMs"]["repo_code_scan"] == pytest.approx(0.0002)
    assert observability["stageElapsedMs"]["cortex_node_spawn"] > 0


def test_provider_passes_central_generation_to_cortex_cli(monkeypatch, tmp_path):
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    observed = {}
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        cortex,
        "_read_manifest",
        lambda _root: {"generationId": "sha256:" + "1" * 64},
    )

    def run(command, **_kwargs):
        observed["command"] = command
        return SimpleNamespace(
            returncode=0,
            stdout=json.dumps({"schemaVersion": 1, "candidates": [{"id": "x", "kind": "symbol",
                "name": "x", "qualifiedName": "x", "path": "x.rs", "confidence": 1.0,
                "evidence": [{"path": "x.rs", "startLine": 1, "endLine": 1, "contentHash": "a" * 32}]}]}),
            stderr="",
        )

    monkeypatch.setattr(cortex.subprocess, "run", run)
    expected = "sha256:" + "1" * 64
    _candidates, generation, warnings = cortex.produce(
        tmp_path, "task", 4096, expected_generation=expected
    )

    assert generation == expected
    assert warnings == []
    index = observed["command"].index("--expected-generation")
    assert observed["command"][index + 1] == expected



def test_pinned_generation_always_uses_cli_lane(monkeypatch, tmp_path):
    """Plan 3.2: the direct sqlite3 lane is deleted. All candidate requests
    route through cortex.mjs graph candidates, even for pinned generations."""
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    generation = "xxh128:" + "1" * 32
    evidence = [{"path": "src/bounded_runtime.rs", "startLine": 10, "endLine": 20,
                  "contentHash": "2" * 32}]
    cli_output = json.dumps({
        "schemaVersion": 1,
        "candidates": [{
            "id": "symbol:bounded", "kind": "symbol", "name": "bounded_runtime",
            "qualifiedName": "bounded_runtime", "path": "src/bounded_runtime.rs",
            "confidence": 1.0, "evidence": evidence,
        }],
    })
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        cortex.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=cli_output, stderr="",
        ),
    )
    monkeypatch.setattr(cortex, "_read_manifest", lambda _root: {"generationId": generation})

    candidates, observed_generation, warnings = cortex.produce(
        tmp_path, "bounded runtime implementation", 64, expected_generation=generation
    )

    assert observed_generation == generation
    assert warnings == []
    assert len(candidates) == 1
    assert candidates[0]["sourceRef"] == "src/bounded_runtime.rs:10-20"
    assert candidates[0]["id"] == "cortex:symbol:bounded"
    assert candidates[0]["sourceHash"] == "sha256:" + "2" * 32 + "0" * 32
    assert candidates[0]["resolver"] == "cortex graph resolve --node symbol:bounded"
    assert candidates[0]["protected"] is False
    assert candidates[0]["text"] == "bounded_runtime"


def test_pinned_generation_cli_abstains_on_no_match(monkeypatch, tmp_path):
    """Plan 3.3: when the CLI returns zero candidates on a pinned generation,
    the provider sets the abstention signal — not an arbitrary cohort."""
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    generation = "xxh128:" + "3" * 32
    cli_output_empty = json.dumps({"schemaVersion": 1, "candidates": []})
    cli_output_match = json.dumps({
        "schemaVersion": 1,
        "candidates": [{
            "id": "symbol:admission", "kind": "symbol", "name": "admission_gate",
            "qualifiedName": "admission_gate", "path": "src/admission.rs",
            "confidence": 1.0,
            "evidence": [{"path": "src/admission.rs", "startLine": 4, "endLine": 9, "contentHash": "4" * 32}],
        }],
    })
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")
    monkeypatch.setattr(cortex, "_read_manifest", lambda _root: {"generationId": generation})

    call_count = {"n": 0}
    def mock_run(*_args, **_kwargs):
        call_count["n"] += 1
        output = cli_output_match if call_count["n"] == 1 else cli_output_empty
        return SimpleNamespace(returncode=0, stdout=output, stderr="")
    monkeypatch.setattr(cortex.subprocess, "run", mock_run)

    matched, observed_generation, warnings = cortex.produce(
        tmp_path, "admission implementation", 1, expected_generation=generation
    )
    abstained, abstained_generation, abstained_warnings = cortex.produce(
        tmp_path, "termthatdoesnotexist", 1, expected_generation=generation
    )

    assert observed_generation == abstained_generation == generation
    assert warnings == []
    assert [candidate["id"] for candidate in matched] == ["cortex:symbol:admission"]
    assert abstained == []
    abstention_kinds = [warning["kind"] for warning in abstained_warnings]
    assert "cortex_abstained_no_relevant_seed" in abstention_kinds


def test_provider_terminates_cortex_at_lane_deadline(monkeypatch, tmp_path):
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    observed = {}
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")

    def run(_command, **kwargs):
        observed["timeout"] = kwargs["timeout"]
        raise cortex.subprocess.TimeoutExpired("node", kwargs["timeout"])

    monkeypatch.setattr(cortex.subprocess, "run", run)

    candidates, generation, warnings = cortex.produce(tmp_path, "task", 4096)

    assert candidates == []
    assert generation == "cortex-timeout"
    assert warnings[0]["kind"] == "provider_timeout"
    assert observed["timeout"] == cortex.CORTEX_TIMEOUT_S


def test_timeout_reuses_only_exact_fresh_candidate_cache(monkeypatch, tmp_path):
    cli = tmp_path / "cortex.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    expected = "generation-1"
    payload = {
        "schemaVersion": 1,
        "candidates": [{
            "id": "node-1",
            "sourceRef": "src/app.py:1-2",
            "sourceHash": "sha256:" + "0" * 64,
            "qualifiedName": "app.main",
        }],
    }
    cortex._candidate_cache.clear()
    monkeypatch.setattr(cortex, "_resolve_cortex_cli", lambda: str(cli))
    monkeypatch.setattr(cortex, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        cortex,
        "_read_manifest",
        lambda _root: {"generationId": expected},
    )
    monkeypatch.setattr(
        cortex.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(payload), stderr=""
        ),
    )
    cached, generation, warnings = cortex.produce(
        tmp_path, "task-a", 64, expected_generation=expected
    )
    assert cached and generation == expected and warnings == []

    def timeout(_command, **kwargs):
        raise cortex.subprocess.TimeoutExpired("node", kwargs["timeout"])

    monkeypatch.setattr(cortex.subprocess, "run", timeout)
    reused, generation, warnings = cortex.produce(
        tmp_path, "task-a", 64, expected_generation=expected
    )
    assert reused == cached
    assert generation == expected
    assert warnings == []

    missing, generation, warnings = cortex.produce(
        tmp_path, "task-b", 64, expected_generation=expected
    )
    assert missing == []
    assert generation == "cortex-timeout"
    assert warnings[0]["kind"] == "provider_timeout"


def test_manifest_reads_generation_from_manifest_json(tmp_path):
    """Plan 3.2: _read_manifest now reads from manifest.json, not graph.db."""
    graph_dir = tmp_path / ".agent" / "graph"
    graph_dir.mkdir(parents=True)
    manifest = {"generationId": "generation-json", "baseCommit": "commit-json", "sourceState": "clean"}
    (graph_dir / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")

    result = cortex._read_manifest(tmp_path)

    assert result["generationId"] == "generation-json"
