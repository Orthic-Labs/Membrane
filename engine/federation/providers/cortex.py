"""Cortex provider — reads durable candidates from Membrane's resident service.

Per dispatch: Cortex remains durable memory only. This provider reads
eligible durable rows via Membrane's authenticated resident route and emits them as Layer 7 candidates
for the planner.
"""
from __future__ import annotations

import json
import math
import os
import urllib.request
from pathlib import Path


_MEMORY_STAGE_NAMES = frozenset({"request_parse", "embed", "recall", "rank"})
# Match the gateway's production fanout ceiling so this lane is not cancelled early.
_PRODUCTION_SERVE_TIMEOUT_S = 0.35
_REPLAY_SERVE_TIMEOUT_S = 45.0


def _replay_no_legacy() -> bool:
    return (
        os.environ.get("REPLAY_NO_LEGACY", "").strip() == "1"
        and os.environ.get("MEMBRANE_SAMPLE_SOURCE", "").strip().lower()
        in {"replay", "test"}
    )


def _stage_observability(ccs: object) -> dict:
    """Return only the bounded phase-0 timing subset; never forward response content."""
    if not isinstance(ccs, dict):
        return {"stageElapsedMs": {}}
    membrane = ccs.get("_membrane")
    raw = membrane.get("stageElapsedMs") if isinstance(membrane, dict) else None
    stages = {}
    if isinstance(raw, dict):
        for name, elapsed in raw.items():
            if name not in _MEMORY_STAGE_NAMES or isinstance(elapsed, bool):
                continue
            if isinstance(elapsed, (int, float)):
                numeric = float(elapsed)
                if math.isfinite(numeric) and numeric >= 0:
                    stages[name] = numeric
    return {"stageElapsedMs": stages}


def _ccs_from_serve(task: str, scope: str, max_candidates: int, scope_descriptor: dict | None = None) -> dict | None:
    """Ask the WARM resident serve for memory candidates (in-process, no ONNX reload). Returns the
    parsed CCS dict, or None if the serve is unreachable/errors so the caller cold-spawns. This is
    the on-mode latency fix: the cold `cortex memory-candidates` CLI reloads fastembed (~3.6s) and
    blew the hook timeout; the serve embedder is already loaded."""
    port = os.environ.get("MEMBRANE_PORT") or "47851"
    token = ""
    token_file = os.environ.get("MEMBRANE_API_TOKEN_FILE", "")
    try:
        if token_file and os.path.exists(token_file):
            with open(token_file, "r", encoding="utf-8") as handle:
                token = handle.read().strip()
    except OSError:
        token = ""
    payload = {"task": task, "scope": scope, "max_candidates": max_candidates}
    if scope_descriptor is not None:
        payload["scopeDescriptor"] = scope_descriptor
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/memory-candidates",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"},
    )
    try:
        timeout_s = (
            _REPLAY_SERVE_TIMEOUT_S
            if _replay_no_legacy()
            else _PRODUCTION_SERVE_TIMEOUT_S
        )
        with urllib.request.urlopen(req, timeout=timeout_s) as resp:
            return json.loads(resp.read().decode("utf-8", errors="replace"))
    except Exception:
        return None


def produce_with_observability(
    repo_root: Path, task: str, scope_grant_id: str | None, scope_descriptor: dict | None = None
) -> tuple[list[dict], str, dict]:
    # Prompt path is resident-only. Cold ONNX startup is diagnostics/setup work, never recall work.
    ccs = _ccs_from_serve(task, str(repo_root), 64, scope_descriptor)
    source = "membrane-resident-cortex"
    if ccs is None:
        if _replay_no_legacy():
            raise RuntimeError("replay Membrane resident unavailable")
        raise RuntimeError("Membrane resident unavailable")
    raw_candidates = ccs.get("candidates", []) if isinstance(ccs, dict) else []
    candidates = []
    for c in raw_candidates:
        candidates.append({
            "id": c.get("id"),
            "layer": int(c.get("layer", 7)),
            "sourceKind": c.get("sourceKind", "memory"),
            "sourceRef": c.get("sourceRef", ""),
            "sourceHash": c.get("sourceHash", "0" * 64),
            "trustClass": c.get("trustClass", "agent_verified"),
            "instructionPolicy": c.get("instructionPolicy", "data_only"),
            "providerScore": float(c.get("providerScore", 0.5)),
            "scoreComponents": c.get("scoreComponents", {}),
            "estimatedTokens": int(c.get("estimatedTokens", 80)),
            "protected": False,
            "exact": False,
            "recoverable": True,
            "resolver": c.get("resolver", ""),
            "text": c.get("text", c.get("id", "")),
        })
    return candidates, source, _stage_observability(ccs)


def produce(repo_root: Path, task: str, scope_grant_id: str | None, scope_descriptor: dict | None = None) -> tuple[list[dict], str]:
    """Compatibility wrapper for callers using the historical two-tuple contract."""
    candidates, source, _observability = produce_with_observability(
        repo_root, task, scope_grant_id, scope_descriptor
    )
    return candidates, source
