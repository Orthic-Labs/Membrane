import json
import sqlite3
from types import SimpleNamespace

import pytest

from federation.providers import blueprint
from federation.providers.blueprint import candidate_cap


def test_cli_resolution_prefers_cortex_and_retains_legacy_fallback(monkeypatch, tmp_path):
    cortex_cli = tmp_path / "cortex" / "blueprint.mjs"
    legacy_cli = tmp_path / "blueprint" / "blueprint.mjs"
    cortex_cli.parent.mkdir()
    legacy_cli.parent.mkdir()
    cortex_cli.touch()
    legacy_cli.touch()
    monkeypatch.delenv("BLUEPRINT_CLI", raising=False)
    monkeypatch.setattr(blueprint, "CORTEX_CLI_DEFAULT", cortex_cli)
    monkeypatch.setattr(blueprint, "BLUEPRINT_CLI_DEFAULT", legacy_cli)

    assert blueprint._resolve_blueprint_cli() == str(cortex_cli)
    cortex_cli.unlink()
    assert blueprint._resolve_blueprint_cli() == str(legacy_cli)


def test_repo_code_candidate_cap_is_independent_of_large_context_budget():
    assert candidate_cap(2048, None) == 64
    assert candidate_cap(4096, None) == 64


def test_repo_code_candidate_cap_is_bounded_and_invalid_override_is_safe():
    assert candidate_cap(4096, "32") == 32
    assert candidate_cap(4096, "999") == 256
    assert candidate_cap(4096, "invalid") == 64


def test_observability_splits_subprocess_and_repo_scan_without_changing_produce(monkeypatch, tmp_path):
    cli = tmp_path / "blueprint.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    payload = {
        "schemaVersion": 1,
        "freshness": {"stale": False},
        "_rightcontext": {
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
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        blueprint,
        "_read_manifest",
        lambda _root: {"generationId": "generation-1", "freshness": {"stale": False}},
    )
    monkeypatch.setattr(blueprint.time, "monotonic", lambda: next(monotonic_values))
    monkeypatch.setattr(
        blueprint.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(payload), stderr=""
        ),
    )

    candidates, generation, warnings, observability = blueprint.produce_with_observability(
        tmp_path, "secret task", 4096
    )
    legacy_result = blueprint.produce(tmp_path, "secret task", 4096)

    assert generation == "generation-1"
    assert warnings == []
    assert candidates[0]["id"] == "blueprint:node-1"
    assert observability == {
        "stageElapsedMs": {
            "blueprint_node_spawn": 27.5,
            "repo_code_scan": 12.5,
        }
    }
    assert legacy_result[:2] == (candidates, generation)
    assert legacy_result[2] == []
    assert len(legacy_result) == 3
    assert "secret task" not in json.dumps(observability)


def test_observability_preserves_positive_submillisecond_stage_timing(monkeypatch, tmp_path):
    cli = tmp_path / "blueprint.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    payload = {
        "schemaVersion": 1,
        "_rightcontext": {"stageElapsedMs": {"repo_code_scan": 0.0002}},
        "candidates": [],
    }
    monotonic_values = iter((100.0, 100.0000004))
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        blueprint,
        "_read_manifest",
        lambda _root: {"generationId": "generation-1", "freshness": {"stale": False}},
    )
    monkeypatch.setattr(blueprint.time, "monotonic", lambda: next(monotonic_values))
    monkeypatch.setattr(
        blueprint.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(payload), stderr=""
        ),
    )

    *_result, observability = blueprint.produce_with_observability(
        tmp_path, "task", 4096
    )

    assert observability["stageElapsedMs"]["repo_code_scan"] == pytest.approx(0.0002)
    assert observability["stageElapsedMs"]["blueprint_node_spawn"] > 0


def test_provider_passes_central_generation_to_blueprint_cli(monkeypatch, tmp_path):
    cli = tmp_path / "blueprint.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    observed = {}
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        blueprint,
        "_read_manifest",
        lambda _root: {"generationId": "sha256:" + "1" * 64},
    )

    def run(command, **_kwargs):
        observed["command"] = command
        return SimpleNamespace(
            returncode=0,
            stdout=json.dumps({"schemaVersion": 1, "candidates": []}),
            stderr="",
        )

    monkeypatch.setattr(blueprint.subprocess, "run", run)
    expected = "sha256:" + "1" * 64
    _candidates, generation, warnings = blueprint.produce(
        tmp_path, "task", 4096, expected_generation=expected
    )

    assert generation == expected
    assert warnings == []
    index = observed["command"].index("--expected-generation")
    assert observed["command"][index + 1] == expected


def test_manifest_reads_generation_envelope_from_graph_db(tmp_path):
    graph_dir = tmp_path / ".agent" / "graph"
    graph_dir.mkdir(parents=True)
    with sqlite3.connect(graph_dir / "graph.db") as connection:
        connection.execute("CREATE TABLE generation (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        connection.execute(
            "INSERT INTO generation(key, value) VALUES (?, ?)",
            ("manifest", json.dumps({"generationId": "generation-db"})),
        )
        connection.execute(
            "INSERT INTO generation(key, value) VALUES (?, ?)",
            ("sourceObservation", json.dumps({"head": "commit-db", "dirty": False})),
        )
        connection.commit()

    manifest = blueprint._read_manifest(tmp_path)

    assert manifest == {
        "generationId": "generation-db",
        "baseCommit": "commit-db",
        "sourceState": "clean",
    }
