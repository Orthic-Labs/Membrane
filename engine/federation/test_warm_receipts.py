from __future__ import annotations

import hashlib
import json
import math
import platform
import resource
import subprocess
import time
from pathlib import Path

from federation import gateway


HERE = Path(__file__).parent
ROOT = HERE.parents[1]
FIXTURE = json.loads((HERE / "fixtures" / "warm-federation-v1.json").read_text())
NET = json.loads((HERE / "fixtures" / "equivalence-net-v1.json").read_text())
RELEASE_GENERATION = "sha256:" + "6" * 64


def _semantic_digest(packet: dict) -> str:
    semantic = json.loads(json.dumps(packet))
    semantic["freshness"]["indexedAt"] = "<time>"
    telemetry = semantic["_rightcontext"]
    telemetry["repoRoot"] = "<repo>"
    for field in ("providerElapsedMs", "providerStageElapsedMs", "stageElapsedMs"):
        telemetry.pop(field, None)
    return hashlib.sha256(
        json.dumps(semantic, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def test_warm_receipts_bind_environment_measurements_and_stable_semantics(tmp_path: Path):
    receipts = []
    source_commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()
    providers = NET["providerOrder"]

    for repetition in range(FIXTURE["repetitions"]):
        started = time.monotonic()
        freshness = {
            "indexedAt": "2026-08-12T00:00:00Z",
            "graphState": "clean",
            "generationId": "sha256:" + "2" * 64,
            "expectedReleaseGeneration": RELEASE_GENERATION,
            "observedReleaseGeneration": RELEASE_GENERATION,
            "releaseGenerationStatus": "match",
            "_providerElapsedMs": {provider: 0.0 for provider in providers},
            "_providerStageElapsedMs": {provider: {} for provider in providers},
            "_stageElapsedMs": {"freshness": 0.0, "provider_fanout": 0.0},
        }
        per_provider = [(provider, [], []) for provider in providers]
        packet = gateway._merge_candidates(
            per_provider, freshness, tmp_path, "warm fixture", "trace-warm", 4096
        )
        wall_ms = max(0.0, (time.monotonic() - started) * 1000.0)
        receipt = {
            "schemaVersion": FIXTURE["schemaVersion"],
            "corpus": FIXTURE["corpus"],
            "providerMix": FIXTURE["providerMix"],
            "providerOrder": providers,
            "timeoutSeconds": FIXTURE["timeoutSeconds"],
            "host": {"os": platform.system(), "arch": platform.machine()},
            "toolchains": {"python": platform.python_version()},
            "sourceCommit": source_commit,
            "releaseGeneration": RELEASE_GENERATION,
            "stageElapsedMs": packet["_rightcontext"]["stageElapsedMs"],
            "providerElapsedMs": packet["_rightcontext"]["providerElapsedMs"],
            "wallElapsedMs": wall_ms,
            "peakRss": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
            "semanticDigest": _semantic_digest(packet),
            "warningCount": len(packet["_rightcontext"]["providerWarnings"]),
            "omissionCount": len(packet["omissions"]),
            "exitCode": 0,
            "repetition": repetition + 1,
        }
        assert list(receipt) == FIXTURE["receiptKeys"]
        for measurement in (receipt["wallElapsedMs"], receipt["peakRss"]):
            assert isinstance(measurement, (int, float))
            assert math.isfinite(measurement) and measurement >= 0
        assert all(
            math.isfinite(value) and value >= 0
            for value in receipt["providerElapsedMs"].values()
        )
        receipts.append(receipt)

    assert len({receipt["semanticDigest"] for receipt in receipts}) == 1
    assert all(receipt["providerOrder"] == providers for receipt in receipts)
