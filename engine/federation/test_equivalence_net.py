from __future__ import annotations

import json
from pathlib import Path

import pytest

from federation import gateway


FIXTURE = json.loads(
    (Path(__file__).parent / "fixtures" / "equivalence-net-v1.json").read_text()
)


def _candidate(candidate_id: str) -> dict:
    return {
        "id": candidate_id,
        "sourceKind": "repo_code",
        "sourceRef": "fixture:1",
        "sourceHash": "sha256:" + "1" * 64,
        "trustClass": "workspace_tracked",
        "instructionPolicy": "data_only",
        "providerScore": 1.0,
        "scoreComponents": {"structural": 1.0},
        "estimatedTokens": 1,
        "protected": False,
        "exact": True,
        "recoverable": True,
        "resolver": "fixture",
        "text": "fixture",
    }


def _freshness() -> dict:
    return {
        "indexedAt": "2026-08-12T00:00:00Z",
        "stale": False,
        "graphState": "clean",
        "generationId": "sha256:" + "2" * 64,
        "snapshotId": "sha256:" + "3" * 64,
        "baseCommit": "4" * 40,
        "overlayDigest": "sha256:" + "5" * 64,
        "overlayIdentity": "fixture-overlay",
        "expectedReleaseGeneration": "sha256:" + "6" * 64,
        "observedReleaseGeneration": "sha256:" + "6" * 64,
        "releaseGenerationStatus": "match",
        "cacheAgeMs": 0.0,
        "refreshInFlight": False,
        "_providerElapsedMs": {},
        "_providerStageElapsedMs": {},
        "_stageElapsedMs": {},
    }


def test_packet_shape_order_precedence_dedup_freshness_and_typed_degradation(tmp_path: Path):
    providers = FIXTURE["providerOrder"]
    per_provider = []
    for index, provider in enumerate(providers):
        candidate_id = "shared" if index < 2 else f"{provider}:fixture"
        warnings = []
        if index < len(FIXTURE["warningKinds"]):
            kind = list(FIXTURE["warningKinds"])[index]
            warnings.append({
                "provider": provider,
                "kind": kind,
                "severity": "warning",
                "message": "SECRET detail must not cross boundary",
            })
        per_provider.append((provider, [_candidate(candidate_id)], warnings))

    packet = gateway._merge_candidates(
        per_provider, _freshness(), tmp_path, "fixture", "trace-fixture", 4096
    )

    assert list(packet) == FIXTURE["candidateSetKeys"]
    assert [candidate["provider"] for candidate in packet["candidates"]] == [
        providers[0], *providers[2:]
    ]
    assert list(packet["freshness"]) == FIXTURE["freshnessKeys"]
    assert packet["freshness"]["releaseGenerationStatus"] == "match"
    warnings = packet["_rightcontext"]["providerWarnings"]
    assert [warning["provider"] for warning in warnings] == providers
    assert [warning["failureKind"] for warning in warnings] == list(
        FIXTURE["warningKinds"].values()
    )
    assert all(warning["failureScope"] == "lane_local" for warning in warnings)
    assert "SECRET" not in json.dumps(packet)


def test_same_root_without_grant_fans_out(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    calls = []
    monkeypatch.setattr(gateway, "_enforce_scope_grant", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(
        gateway,
        "_gather_all_parallel",
        lambda **_kwargs: (calls.append("fanout") or [], _freshness()),
    )

    packet, exit_code = gateway.assemble_candidate_set(task="fixture", repo=str(tmp_path))

    assert exit_code == 0
    assert calls == ["fanout"]
    assert packet["candidates"] == []


def test_invalid_grant_aborts_before_fanout(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    def reject(*_args, **_kwargs):
        raise PermissionError("scope_grant_invalid: fixture")

    def forbidden(**_kwargs):
        raise AssertionError("fanout must not start")

    monkeypatch.setattr(gateway, "_enforce_scope_grant", reject)
    monkeypatch.setattr(gateway, "_gather_all_parallel", forbidden)

    packet, exit_code = gateway.assemble_candidate_set(
        task="fixture", repo=str(tmp_path), scope_grant_id="fixture-grant", session="fixture"
    )

    assert exit_code == 2
    assert packet["candidates"] == []
    assert packet["omissions"][0]["severity"] == "blocker"
    assert packet["_rightcontext"]["abortReason"] == "scope_grant"
