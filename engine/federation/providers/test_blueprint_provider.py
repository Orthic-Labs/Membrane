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


def test_pinned_generation_queries_sqlite_without_node_spawn(monkeypatch, tmp_path):
    # Plan 3.3 (defect 4): _pin_fresh_candidate was deleted. Earlier this test
    # asserted `protected is True` and `text == "Cortex source range ..."`,
    # but those came from the retired protection step. The candidate now
    # reflects only what name-equality produced.
    graph_dir = tmp_path / ".agent" / "graph"
    graph_dir.mkdir(parents=True)
    generation = "xxh128:" + "1" * 32
    evidence = json.dumps([{
        "path": "src/bounded_runtime.rs", "startLine": 10, "endLine": 20,
        "contentHash": "2" * 32,
    }])
    with sqlite3.connect(graph_dir / "graph.db") as connection:
        connection.execute("CREATE TABLE generation (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        connection.execute("INSERT INTO generation VALUES ('manifest', ?)", (json.dumps({"generationId": generation}),))
        connection.execute(
            "CREATE TABLE symbols (id TEXT PRIMARY KEY, kind TEXT, labels TEXT, name TEXT, qualified_name TEXT, path TEXT, confidence REAL, evidence TEXT, generation_id TEXT, extra TEXT)"
        )
        connection.execute(
            "CREATE TABLE files (path TEXT PRIMARY KEY, content_hash TEXT, language TEXT, provider TEXT, parse_status TEXT, error_node_count INTEGER, generation_id TEXT, node_id TEXT, labels TEXT, name TEXT, qualified_name TEXT, confidence REAL, evidence TEXT, extra TEXT)"
        )
        connection.execute(
            "INSERT INTO symbols VALUES (?,?,?,?,?,?,?,?,?,?)",
            ("symbol:bounded", "symbol", '["Function"]', "bounded_runtime", "bounded_runtime", "src/bounded_runtime.rs", 1.0, evidence, generation, "{}"),
        )
    cli = tmp_path / "blueprint.mjs"
    cli.write_text("// must not run", encoding="utf-8")
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        blueprint.subprocess,
        "run",
        lambda *_args, **_kwargs: pytest.fail("pinned direct index path spawned Node"),
    )

    candidates, observed_generation, warnings = blueprint.produce(
        tmp_path, "bounded runtime implementation", 64, expected_generation=generation
    )

    assert observed_generation == generation
    assert warnings == []
    assert candidates[0]["sourceRef"] == "src/bounded_runtime.rs:10-20"
    assert candidates[0]["id"] == "blueprint:symbol:bounded"
    # Plan 3.4 hash prefix: emitted digest is always `sha256:` + 64-hex.
    assert candidates[0]["sourceHash"] == "sha256:" + "2" * 32 + "0" * 32
    # Plan 3.4 resolver drift: the published re-fetch shape names the same
    # command actually executed elsewhere (`blueprint graph resolve --node`).
    assert candidates[0]["resolver"] == "blueprint graph resolve --node symbol:bounded"
    # Plan 3.3 defect 4 retired `_pin_fresh_candidate`, so neither the
    # `protected` flag nor the rewrites `text` survive past the SQLite read.
    assert candidates[0]["protected"] is False
    assert candidates[0]["text"] == "bounded_runtime"


def test_pinned_generation_uses_fts_and_abstains_on_no_match(monkeypatch, tmp_path):
    # Plan 3.3 (defect 3): the legacy deterministic arbitrary-cohort fallback
    # has been retired. A query whose tokens produce zero symbols under name
    # equality now abstains (zero candidates + abstention warning), it does
    # NOT return a `src/`-prefixed arbitrary cohort.
    graph_dir = tmp_path / ".agent" / "graph"
    graph_dir.mkdir(parents=True)
    generation = "xxh128:" + "3" * 32
    evidence = json.dumps([{
        "path": "src/admission.rs", "startLine": 4, "endLine": 9,
        "contentHash": "4" * 32,
    }])
    with sqlite3.connect(graph_dir / "graph.db") as connection:
        connection.execute("CREATE TABLE generation (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        connection.execute("INSERT INTO generation VALUES ('manifest', ?)", (json.dumps({"generationId": generation}),))
        connection.execute(
            "CREATE TABLE symbols (id TEXT PRIMARY KEY, kind TEXT, labels TEXT, name TEXT, qualified_name TEXT, path TEXT, confidence REAL, evidence TEXT, generation_id TEXT, extra TEXT)"
        )
        connection.execute(
            "CREATE VIRTUAL TABLE symbol_search USING fts5(id UNINDEXED, generation_id UNINDEXED, name, qualified_name, path)"
        )
        connection.execute(
            "CREATE TABLE symbol_terms (generation_id TEXT, token TEXT, symbol_id TEXT, PRIMARY KEY (generation_id, token, symbol_id)) WITHOUT ROWID"
        )
        rows = [
            ("symbol:admission", "admission_gate", "admission_gate", "src/admission.rs"),
            ("symbol:capsule", "capsule_verify", "capsule_verify", "src/capsule.rs"),
        ]
        for entry_id, name, qualified_name, path in rows:
            connection.execute(
                "INSERT INTO symbols VALUES (?,?,?,?,?,?,?,?,?,?)",
                (entry_id, "symbol", '["Function"]', name, qualified_name, path, 1.0, evidence.replace("src/admission.rs", path), generation, "{}"),
            )
            connection.execute(
                "INSERT INTO symbol_search VALUES (?,?,?,?,?)",
                (entry_id, generation, name.replace("_", " "), qualified_name.replace("_", " "), path),
            )
            for token in ("*", name, *name.split("_")):
                connection.execute(
                    "INSERT OR IGNORE INTO symbol_terms VALUES (?,?,?)",
                    (generation, token, entry_id),
                )
    cli = tmp_path / "blueprint.mjs"
    cli.write_text("// must not run", encoding="utf-8")
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        blueprint.subprocess,
        "run",
        lambda *_args, **_kwargs: pytest.fail("pinned FTS path spawned Node"),
    )

    matched, observed_generation, warnings = blueprint.produce(
        tmp_path, "admission implementation", 1, expected_generation=generation
    )
    abstained, abstained_generation, abstained_warnings = blueprint.produce(
        tmp_path, "termthatdoesnotexist", 1, expected_generation=generation
    )

    assert observed_generation == abstained_generation == generation
    # Matched query: returns the symbol it found and emits no warnings.
    assert warnings == []
    assert [candidate["id"] for candidate in matched] == ["blueprint:symbol:admission"]
    # Plan 3.3 abstention: pinned graph + no lexical hit = empty candidates
    # + an abstention-typed warning.
    assert abstained == []
    abstention_kinds = [warning["kind"] for warning in abstained_warnings]
    assert "blueprint_abstained_no_relevant_seed" in abstention_kinds


def test_provider_terminates_blueprint_at_lane_deadline(monkeypatch, tmp_path):
    cli = tmp_path / "blueprint.mjs"
    cli.write_text("// fixture", encoding="utf-8")
    observed = {}
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")

    def run(_command, **kwargs):
        observed["timeout"] = kwargs["timeout"]
        raise blueprint.subprocess.TimeoutExpired("node", kwargs["timeout"])

    monkeypatch.setattr(blueprint.subprocess, "run", run)

    candidates, generation, warnings = blueprint.produce(tmp_path, "task", 4096)

    assert candidates == []
    assert generation == "blueprint-timeout"
    assert warnings[0]["kind"] == "provider_timeout"
    assert observed["timeout"] == blueprint.BLUEPRINT_TIMEOUT_S


def test_timeout_reuses_only_exact_fresh_candidate_cache(monkeypatch, tmp_path):
    cli = tmp_path / "blueprint.mjs"
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
    blueprint._candidate_cache.clear()
    monkeypatch.setattr(blueprint, "_resolve_blueprint_cli", lambda: str(cli))
    monkeypatch.setattr(blueprint, "_resolve_node", lambda: "node")
    monkeypatch.setattr(
        blueprint,
        "_read_manifest",
        lambda _root: {"generationId": expected},
    )
    monkeypatch.setattr(
        blueprint.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(payload), stderr=""
        ),
    )
    cached, generation, warnings = blueprint.produce(
        tmp_path, "task-a", 64, expected_generation=expected
    )
    assert cached and generation == expected and warnings == []

    def timeout(_command, **kwargs):
        raise blueprint.subprocess.TimeoutExpired("node", kwargs["timeout"])

    monkeypatch.setattr(blueprint.subprocess, "run", timeout)
    reused, generation, warnings = blueprint.produce(
        tmp_path, "task-a", 64, expected_generation=expected
    )
    assert reused == cached
    assert generation == expected
    assert warnings == []

    missing, generation, warnings = blueprint.produce(
        tmp_path, "task-b", 64, expected_generation=expected
    )
    assert missing == []
    assert generation == "blueprint-timeout"
    assert warnings[0]["kind"] == "provider_timeout"


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
