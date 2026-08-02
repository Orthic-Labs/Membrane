from __future__ import annotations

import copy
import hashlib
import json
import os
import subprocess
from pathlib import Path

from federation import gateway


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FIXTURE = json.loads(
    (HERE / "fixtures" / "provider-merge-order-v1.json").read_text(encoding="utf-8")
)
RELEASE_GENERATION = "sha256:" + "4" * 64


def _candidate(provider: str, precedence: int) -> dict:
    return {
        "id": f"{provider}:fixture",
        "layer": 3,
        "sourceKind": "repo_code",
        "sourceRef": f"fixtures/{provider}.txt:1",
        "sourceHash": "sha256:" + hashlib.sha256(provider.encode()).hexdigest(),
        "trustClass": "workspace_tracked",
        "instructionPolicy": "data_only",
        "providerScore": 1.0 - (precedence / 100.0),
        "scoreComponents": {"structural": 1.0},
        "estimatedTokens": 10,
        "protected": False,
        "exact": True,
        "recoverable": True,
        "resolver": f"read fixtures/{provider}.txt",
        "text": f"Fixture evidence from {provider}",
    }


def _warning(provider: str) -> dict:
    return {
        "provider": provider,
        "kind": "fixture_warning",
        "severity": "warning",
        "message": f"{provider} fixture warning",
    }


class _ImmediateFuture:
    def __init__(self, value):
        self._value = value

    def result(self):
        return self._value


class _ImmediateExecutor:
    def __init__(self, **_kwargs):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def submit(self, fn):
        return _ImmediateFuture(fn())


def _planner_command(candidate_set: Path) -> list[str]:
    override = os.environ.get("CRYPT_BIN", "").strip()
    if override:
        return [override, "plan-context", "--candidate-set", str(candidate_set), "--max-tokens", "4096"]

    binary = ROOT / "membrane" / "engine" / "target" / "debug" / (
        "crypt.exe" if os.name == "nt" else "crypt"
    )
    if binary.is_file():
        return [str(binary), "plan-context", "--candidate-set", str(candidate_set), "--max-tokens", "4096"]

    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(ROOT / "membrane" / "engine" / "Cargo.toml"),
        "-p",
        "crypt",
        "--bin",
        "crypt",
        "--",
        "plan-context",
        "--candidate-set",
        str(candidate_set),
        "--max-tokens",
        "4096",
    ]


def _semantic_candidate_set(ccs: dict) -> dict:
    semantic = copy.deepcopy(ccs)
    telemetry = semantic.get("_rightcontext", {})
    for field in ("providerElapsedMs", "providerStageElapsedMs", "stageElapsedMs"):
        telemetry.pop(field, None)
    return semantic


def test_planner_fallback_selects_crypt_cli_binary(monkeypatch, tmp_path: Path):
    monkeypatch.delenv("CRYPT_BIN", raising=False)
    monkeypatch.setitem(globals(), "ROOT", tmp_path)

    command = _planner_command(tmp_path / "candidate-set.json")

    assert command[command.index("--bin") + 1] == "crypt"


def test_completion_permutations_keep_precedence_receipts_and_packet_hash_stable(
    monkeypatch, tmp_path: Path
):
    declared_order = FIXTURE["declaredProviderOrder"]
    candidates = {
        provider: _candidate(provider, index)
        for index, provider in enumerate(declared_order)
    }
    verdict = {
        "checkedAt": "2026-07-17T00:00:00Z",
        "graphState": "clean",
        "snapshotId": "sha256:" + "1" * 64,
        "baseCommit": "a" * 40,
        "blueprintBaseCommit": "a" * 40,
        "blueprintGeneration": "sha256:" + "2" * 64,
        "skillsGeneration": "sha256:" + "3" * 64,
        "serviceGeneration": "fixture-service-generation",
        "releaseGeneration": RELEASE_GENERATION,
        "providers": {
            "blueprint": {"freshnessClass": "committed_snapshot", "usable": True},
            "dirtyOverlay": {"freshnessClass": "dirty_overlay", "usable": True},
            "skills": {"freshnessClass": "current", "usable": True},
        },
    }
    monkeypatch.setattr(gateway, "_expected_release_generation", lambda: RELEASE_GENERATION)
    monkeypatch.setattr(gateway, "_fetch_freshness_verdict", lambda _repo: verdict)
    monkeypatch.setattr(gateway.cf, "ThreadPoolExecutor", _ImmediateExecutor)
    monkeypatch.setattr(
        gateway.audit,
        "produce",
        lambda *_args: ([candidates["audit"]], "fixture", [_warning("audit")]),
    )
    monkeypatch.setattr(gateway.architect, "produce", lambda *_args: [candidates["architect"]])
    monkeypatch.setattr(
        gateway.crypt,
        "produce_with_observability",
        lambda *_args: ([candidates["crypt"]], "fixture", {"stageElapsedMs": {}}),
    )
    monkeypatch.setattr(gateway.git_provider, "produce", lambda *_args: [candidates["git"]])
    monkeypatch.setattr(gateway.live, "produce", lambda *_args, **_kwargs: [candidates["live"]])
    monkeypatch.setattr(gateway.rules, "produce", lambda *_args: [candidates["rules"]])
    monkeypatch.setattr(gateway.anchors, "produce", lambda *_args: [candidates["anchors"]])
    monkeypatch.setattr(
        gateway.blueprint,
        "produce_with_observability",
        lambda *_args, **_kwargs: (
            [candidates["blueprint"]],
            verdict["blueprintGeneration"],
            [],
            {"stageElapsedMs": {}},
        ),
    )
    monkeypatch.setattr(
        gateway.skills,
        "produce",
        lambda *_args: (
            [candidates["skills"]],
            verdict["skillsGeneration"],
            [_warning("skills")],
        ),
    )

    semantic_hashes = []
    packet_receipt_hashes = []
    for case_index, completion_order in enumerate(FIXTURE["completionOrders"]):
        rank = {provider: index for index, provider in enumerate(completion_order)}
        monkeypatch.setattr(
            gateway.cf,
            "as_completed",
            lambda futures, rank=rank: sorted(
                futures, key=lambda future: rank[future.result()[0]]
            ),
        )
        per_provider, freshness = gateway._gather_all_parallel(
            task="merge-order fixture",
            repo_root=tmp_path,
            max_tokens=4096,
            explicit_anchors=[],
            scope_grant_id=None,
        )
        assert [provider for provider, _candidates, _warnings in per_provider] == declared_order

        ccs = gateway._merge_candidates(
            per_provider,
            freshness,
            tmp_path,
            "merge-order fixture",
            "00000000-0000-4000-8000-000000000031",
            4096,
        )
        assert [candidate["provider"] for candidate in ccs["candidates"]] == declared_order
        assert [warning["provider"] for warning in ccs["_rightcontext"]["providerWarnings"]] == FIXTURE[
            "warningProviderOrder"
        ]

        semantic = _semantic_candidate_set(ccs)
        semantic_hashes.append(
            hashlib.sha256(
                json.dumps(semantic, sort_keys=True, separators=(",", ":")).encode()
            ).hexdigest()
        )
        candidate_set_path = tmp_path / f"merge-order-{case_index}.json"
        candidate_set_path.write_text(json.dumps(semantic), encoding="utf-8")
        planned = subprocess.run(
            _planner_command(candidate_set_path),
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        assert planned.returncode == 0, planned.stdout + planned.stderr
        output = json.loads(planned.stdout)
        assert [receipt["provider"] for receipt in output["receipts"]] == declared_order
        packet_receipt_hashes.append(
            hashlib.sha256(
                json.dumps(
                    {"packet": output["packet"], "receipts": output["receipts"]},
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode()
            ).hexdigest()
        )

    assert len(set(semantic_hashes)) == 1
    assert len(set(packet_receipt_hashes)) == 1
