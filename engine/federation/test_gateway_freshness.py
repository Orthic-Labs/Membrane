import json
from contextlib import contextmanager
from pathlib import Path

import pytest

from federation import gateway


RELEASE_GENERATION = "sha256:" + "7" * 64


def test_freshness_uses_workspace_default_token_file(monkeypatch, tmp_path: Path):
    workspace = tmp_path / "workspace"
    token_file = workspace / "tools" / ".cache" / "memory" / "api-token"
    token_file.parent.mkdir(parents=True)
    token_file.write_text("default-token\n", encoding="utf-8")
    monkeypatch.delenv("CRYPT_API_TOKEN_FILE", raising=False)
    monkeypatch.setenv("WORKSPACE_ROOT", str(workspace))

    class Response:
        def read(self):
            return json.dumps({"schemaVersion": 1, "graphState": "fresh"}).encode()

    @contextmanager
    def urlopen(request, timeout):
        assert request.headers["Authorization"] == "Bearer default-token"
        assert timeout == 2.0
        yield Response()

    monkeypatch.setattr(gateway.urllib.request, "urlopen", urlopen)
    assert gateway._fetch_freshness_verdict(workspace) == {
        "schemaVersion": 1,
        "graphState": "fresh",
    }


@pytest.fixture(autouse=True)
def _stable_release_manifest(monkeypatch):
    monkeypatch.setattr(
        gateway, "_expected_release_generation", lambda: RELEASE_GENERATION, raising=False
    )


def _verdict(graph_state: str = "dirty_overlay", *, cortex_usable: bool = False) -> dict:
    return {
        "schemaVersion": 1,
        "checkedAt": "2026-07-17T00:00:00Z",
        "serviceGeneration": "svc-test-generation",
        "releaseGeneration": RELEASE_GENERATION,
        "firstAfterIdle": True,
        "stageElapsedMs": {"git_status": 4},
        "snapshotId": "sha256:" + "c" * 64,
        "stable": True,
        "attempts": 1,
        "graphState": graph_state,
        "headCommit": "a" * 40,
        "baseCommit": "a" * 40,
        "cortexBaseCommit": "9" * 40,
        "manifestDigest": "sha256:" + "d" * 64,
        "cortexGeneration": "sha256:" + "e" * 64,
        "skillsGeneration": "sha256:" + "f" * 64,
        "overlayDigest": "sha256:" + "b" * 64,
        "overlayCount": 1,
        "overlayTruncated": False,
        "cacheAgeMs": 37,
        "refreshInFlight": True,
        "overlayEntries": [
            {"path": "src/app.py", "status": " M", "contentHash": "sha256:" + "1" * 64}
        ],
        "providers": {
            "cortex": {"freshnessClass": "stale_snapshot", "usable": cortex_usable},
            "dirtyOverlay": {"freshnessClass": "dirty_overlay", "usable": True},
            "skills": {"freshnessClass": "current", "usable": True},
        },
        "reasons": ["working_tree_dirty"],
    }


def _stub_non_cortex_providers(monkeypatch) -> None:
    monkeypatch.setattr(gateway.audit, "produce", lambda *_args: [])
    monkeypatch.setattr(gateway.architect, "produce", lambda *_args: [])
    monkeypatch.setattr(gateway.crypt, "produce", lambda *_args: ([{"id": "memory:one"}], "g"))
    monkeypatch.setattr(
        gateway.crypt,
        "produce_with_observability",
        lambda *_args: ([{"id": "memory:one"}], "g", {"stageElapsedMs": {}}),
    )
    monkeypatch.setattr(gateway.git_provider, "produce", lambda *_args: [])
    monkeypatch.setattr(gateway.live, "produce", lambda *_args, **_kwargs: [])
    monkeypatch.setattr(gateway.rules, "produce", lambda *_args: [])
    monkeypatch.setattr(gateway.anchors, "produce", lambda *_args: [])
    monkeypatch.setattr(gateway.skills, "produce", lambda *_args: [])


def test_stale_cortex_is_lane_local_and_other_providers_fan_out(monkeypatch, tmp_path: Path):
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: _verdict("stale_snapshot"), raising=False)
    monkeypatch.setattr(
        gateway.cortex,
        "produce",
        lambda *_args: (_ for _ in ()).throw(AssertionError("unusable Cortex lane must be skipped")),
    )
    _stub_non_cortex_providers(monkeypatch)

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    assert any(name == "crypt" for name, _candidates, _warnings in providers)
    cortex_lane = next(item for item in providers if item[0] == "cortex")
    assert cortex_lane[1] == []
    assert cortex_lane[2][0]["kind"] == "cortex_lane_degraded"
    assert freshness["stale"] is False
    assert freshness["graphState"] == "stale_snapshot"
    assert freshness["baseCommit"] == "a" * 40
    assert freshness["serviceGeneration"] == "svc-test-generation"
    assert freshness["expectedReleaseGeneration"] == RELEASE_GENERATION
    assert freshness["observedReleaseGeneration"] == RELEASE_GENERATION
    assert freshness["releaseGenerationStatus"] == "matched"
    assert freshness["firstAfterIdle"] is True
    assert freshness["cacheAgeMs"] == 37
    assert freshness["refreshInFlight"] is True
    assert set(freshness["_stageElapsedMs"]) >= {"freshness", "git_status", "provider_fanout"}
    assert freshness["_stageElapsedMs"]["git_status"] == 4.0
    assert all(value >= 0 for value in freshness["_stageElapsedMs"].values())
    assert set(freshness["_providerElapsedMs"]) == {
        "cortex", "audit", "architect", "crypt", "git", "live", "rules", "anchors", "skills"
    }
    assert all(value >= 0 for value in freshness["_providerElapsedMs"].values())


def test_dirty_worktree_delivers_overlay_and_non_cortex_lanes(monkeypatch, tmp_path: Path):
    verdict = _verdict()
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    monkeypatch.setattr(
        gateway.cortex,
        "produce",
        lambda *_args: (_ for _ in ()).throw(AssertionError("unusable Cortex lane must be skipped")),
    )
    _stub_non_cortex_providers(monkeypatch)
    live_calls = []
    monkeypatch.setattr(
        gateway.live,
        "produce",
        lambda *_args, **kwargs: live_calls.append(kwargs) or [{"id": "git-overlay:src/app.py"}],
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    assert any(name == "live" and candidates for name, candidates, _warnings in providers)
    assert any(name == "crypt" and candidates for name, candidates, _warnings in providers)
    assert freshness["stale"] is False
    assert live_calls == [{
        "base_commit": verdict["baseCommit"],
        "overlay_digest": verdict["overlayDigest"],
        "overlay_entries": verdict["overlayEntries"],
        "prompt_fast": True,
    }]


def test_unavailable_freshness_service_degrades_before_fanout(monkeypatch, tmp_path: Path):
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: None, raising=False)
    monkeypatch.setattr(
        gateway.crypt,
        "produce",
        lambda *_args: (_ for _ in ()).throw(AssertionError("provider must not run")),
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    assert providers == []
    assert freshness["stale"] is True
    assert freshness["expectedReleaseGeneration"] == RELEASE_GENERATION
    assert freshness["observedReleaseGeneration"] is None
    assert freshness["releaseGenerationStatus"] == "unavailable"


def test_release_generation_mismatch_degrades_before_fanout(monkeypatch, tmp_path: Path):
    verdict = _verdict()
    verdict["releaseGeneration"] = "sha256:" + "8" * 64
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    monkeypatch.setattr(
        gateway.crypt,
        "produce_with_observability",
        lambda *_args: (_ for _ in ()).throw(AssertionError("provider must not run")),
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    assert providers == []
    assert freshness["stale"] is True
    assert freshness["expectedReleaseGeneration"] == RELEASE_GENERATION
    assert freshness["observedReleaseGeneration"] == "sha256:" + "8" * 64
    assert freshness["releaseGenerationStatus"] == "mismatch"


def test_invalid_release_manifest_degrades_before_fanout(monkeypatch, tmp_path: Path):
    monkeypatch.setattr(gateway, "_expected_release_generation", lambda: None, raising=False)
    monkeypatch.setattr(
        gateway, "_fetch_freshness_verdict", lambda _repo: _verdict(), raising=False
    )
    monkeypatch.setattr(
        gateway.crypt,
        "produce_with_observability",
        lambda *_args: (_ for _ in ()).throw(AssertionError("provider must not run")),
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    assert providers == []
    assert freshness["releaseGenerationStatus"] == "unavailable"


def test_unusable_skills_lane_is_skipped_without_disabling_other_providers(monkeypatch, tmp_path: Path):
    verdict = _verdict()
    verdict["providers"]["skills"] = {"freshnessClass": "unknown", "usable": False}
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    _stub_non_cortex_providers(monkeypatch)
    monkeypatch.setattr(
        gateway.skills,
        "produce",
        lambda *_args: (_ for _ in ()).throw(AssertionError("unusable skills lane must be skipped")),
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    skills_lane = next(item for item in providers if item[0] == "skills")
    assert skills_lane[1] == []
    assert skills_lane[2][0]["kind"] == "skills_lane_degraded"
    assert any(name == "crypt" for name, _candidates, _warnings in providers)
    assert freshness["stale"] is False


def test_cortex_generation_change_after_verdict_omits_lane(monkeypatch, tmp_path: Path):
    verdict = _verdict(cortex_usable=True)
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    _stub_non_cortex_providers(monkeypatch)
    monkeypatch.setattr(
        gateway.cortex,
        "produce_with_observability",
        lambda *_args, **_kwargs: (
            [{"id": "cortex:mixed"}],
            "sha256:" + "0" * 64,
            [],
            {"stageElapsedMs": {}},
        ),
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    cortex_lane = next(item for item in providers if item[0] == "cortex")
    assert cortex_lane[1] == []
    assert cortex_lane[2][0]["kind"] == "cortex_generation_changed"
    assert any(name == "crypt" for name, _candidates, _warnings in providers)
    assert freshness["stale"] is False


def test_gateway_passes_freshness_generation_into_cortex_provider(monkeypatch, tmp_path: Path):
    verdict = _verdict(cortex_usable=True)
    observed = {}
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    _stub_non_cortex_providers(monkeypatch)

    def produce(_repo, _task, _max_tokens, *, expected_generation=None):
        observed["expected_generation"] = expected_generation
        return [], expected_generation, [], {"stageElapsedMs": {}}

    monkeypatch.setattr(gateway.cortex, "produce_with_observability", produce)

    gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    assert observed["expected_generation"] == verdict["cortexGeneration"]


def test_filesystem_skills_generation_cannot_pass_unchanged_db_verdict(monkeypatch, tmp_path: Path):
    before = _verdict()
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: before, raising=False)
    _stub_non_cortex_providers(monkeypatch)
    monkeypatch.setattr(
        gateway.skills,
        "produce",
        lambda *_args: ([{"id": "skills:filesystem-g2"}], "sha256:" + "0" * 64, []),
    )

    providers, freshness = gateway._gather_all_parallel(
        task="test",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )

    skills_lane = next(item for item in providers if item[0] == "skills")
    assert skills_lane[1] == []
    assert skills_lane[2][0]["kind"] == "skills_generation_changed"
    assert any(name == "crypt" for name, _candidates, _warnings in providers)
    assert freshness["stale"] is False


def test_merge_candidates_stamps_provider_and_freshness_provenance(tmp_path: Path):
    cortex_candidate = {"id": "a", "sourceKind": "repo_code"}
    crypt_candidate = {"id": "b", "sourceKind": "repo_code", "provider": "cortex"}
    merged = gateway._merge_candidates(
        [
            ("cortex", [cortex_candidate], []),
            ("crypt", [crypt_candidate], []),
        ],
        {
            "indexedAt": "2026-07-16T00:00:00Z",
            "stale": False,
            "graphState": "dirty_overlay",
            "baseCommit": "a" * 40,
            "cortexBaseCommit": "9" * 40,
            "overlayDigest": "sha256:" + "b" * 64,
            "snapshotId": "sha256:" + "c" * 64,
            "serviceGeneration": "svc-test-generation",
            "firstAfterIdle": True,
            "cacheAgeMs": 37,
            "refreshInFlight": True,
        },
        tmp_path,
        "test",
        "trace-1",
        4096,
    )

    assert [(c["id"], c["provider"]) for c in merged["candidates"]] == [
        ("a", "cortex"),
        ("b", "crypt"),
    ]
    assert merged["candidates"][0]["freshnessClass"] == "stale_snapshot"
    assert merged["candidates"][1]["freshnessClass"] == "current"
    assert merged["candidates"][0]["baseCommit"] == "9" * 40
    assert merged["candidates"][1]["baseCommit"] == "a" * 40
    assert all(c["overlayDigest"] == "sha256:" + "b" * 64 for c in merged["candidates"])
    assert merged["freshness"]["graphState"] == "dirty_overlay"
    assert merged["freshness"]["cacheAgeMs"] == 37
    assert merged["freshness"]["refreshInFlight"] is True
    assert merged["_rightcontext"]["providerElapsedMs"] == {}
    assert merged["_rightcontext"]["serviceGeneration"] == "svc-test-generation"
    assert merged["_rightcontext"]["firstAfterIdle"] is True
    assert merged["_rightcontext"]["cacheAgeMs"] == 37
    assert merged["_rightcontext"]["refreshInFlight"] is True
    assert merged["_rightcontext"]["stageElapsedMs"] == {}
    assert cortex_candidate == {"id": "a", "sourceKind": "repo_code"}
    assert crypt_candidate == {
        "id": "b",
        "sourceKind": "repo_code",
        "provider": "cortex",
    }


def test_crypt_stage_timing_is_nested_and_content_free(monkeypatch, tmp_path: Path):
    verdict = _verdict()
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    _stub_non_cortex_providers(monkeypatch)
    monkeypatch.setattr(
        gateway.crypt,
        "produce_with_observability",
        lambda *_args: (
            [{"id": "memory:one"}],
            "crypt-serve",
            {
                "stageElapsedMs": {
                    "request_parse": 1.0,
                    "embed": 2.0,
                    "recall": 3.0,
                    "rank": 4.0,
                    "task": "must-not-propagate",
                }
            },
        ),
        raising=False,
    )

    providers, freshness = gateway._gather_all_parallel(
        task="secret prompt content",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )
    merged = gateway._merge_candidates(
        providers, freshness, tmp_path, "secret prompt content", "trace-1", 4096
    )

    assert merged["_rightcontext"]["providerStageElapsedMs"] == {
        "crypt": {
            "request_parse": 1.0,
            "embed": 2.0,
            "recall": 3.0,
            "rank": 4.0,
        }
    }
    assert "must-not-propagate" not in str(merged["_rightcontext"])
    assert "secret prompt content" not in str(merged["_rightcontext"])


def test_cortex_stage_timing_uses_tuple4_observability_and_is_sanitized(
    monkeypatch, tmp_path: Path
):
    verdict = _verdict(cortex_usable=True)
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict, raising=False)
    _stub_non_cortex_providers(monkeypatch)
    monkeypatch.setattr(
        gateway.cortex,
        "produce_with_observability",
        lambda *_args, **_kwargs: (
            [{"id": "cortex:one"}],
            verdict["cortexGeneration"],
            [],
            {
                "stageElapsedMs": {
                    "cortex_node_spawn": 7.0,
                    "repo_code_scan": 11.0,
                    "task": "must-not-propagate",
                }
            },
        ),
        raising=False,
    )

    providers, freshness = gateway._gather_all_parallel(
        task="secret prompt content",
        repo_root=tmp_path,
        max_tokens=4096,
        explicit_anchors=[],
        scope_grant_id=None,
    )
    merged = gateway._merge_candidates(
        providers, freshness, tmp_path, "secret prompt content", "trace-1", 4096
    )

    assert merged["_rightcontext"]["providerStageElapsedMs"]["cortex"] == {
        "cortex_node_spawn": 7.0,
        "repo_code_scan": 11.0,
    }
    assert "must-not-propagate" not in str(merged["_rightcontext"])
    assert "secret prompt content" not in str(merged["_rightcontext"])
