"""Architect provider — invokes the decision lifecycle provider.

The decision_provider module exposes a free function `produce_candidate_set`
(not a class). The signature is
``produce_candidate_set(task, scope=None, *, store=None, store_path=None, mode="edit")``.
We pass the JSONL store path at ``<repo>/.audit/architect/decisions.jsonl``
and a scope dict that contains `repositoryId` + `linkedGraphGeneration`.
"""
from __future__ import annotations

import sys
from pathlib import Path

from . import canonical_repository_id, workspace_tools_path


def produce(repo_root: Path, task: str) -> tuple[list[dict], str]:
    arch_provider_path = (
        workspace_tools_path("skills", "legion", "lib", "decision_provider.py")
    )
    if not arch_provider_path.exists():
        raise FileNotFoundError(f"decision_provider.py missing at {arch_provider_path}")
    sys.path.insert(0, str(arch_provider_path.parent))
    try:
        from decision_provider import produce_candidate_set  # type: ignore
        # decision_provider filters by exact equality on repositoryId, and stored
        # records carry the scope slug (`D--Claude`), not a machine path — sending
        # str(repo_root.resolve()) here matched nothing, so this lane returned zero
        # candidates on every prompt.
        repo_id = canonical_repository_id(repo_root)
        scope = {
            "repositoryId": repo_id,
            "scopeId": repo_id,
            "linkedGraphGeneration": "",
            "traceId": "",
        }
        ccs = produce_candidate_set(
            task=task,
            scope=scope,
            store_path=str(repo_root / ".audit" / "architect" / "decisions.jsonl"),
        )
        if hasattr(ccs, "as_dict"):
            ccs = ccs.as_dict()
    except Exception:
        raise
    finally:
        try:
            sys.path.remove(str(arch_provider_path.parent))
        except ValueError:
            pass

    raw_candidates = ccs.get("candidates", []) if isinstance(ccs, dict) else []
    candidates = []
    for c in raw_candidates:
        candidates.append({
            "id": c.get("id"),
            "layer": int(c.get("layer", 5)),
            "sourceKind": c.get("sourceKind", "architect_decision"),
            "sourceRef": c.get("sourceRef", ""),
            "sourceHash": c.get("sourceHash", "0" * 64),
            "trustClass": c.get("trustClass", "agent_verified"),
            "instructionPolicy": c.get("instructionPolicy", "data_only"),
            "providerScore": float(c.get("providerScore", 0.5)),
            "scoreComponents": c.get("scoreComponents", {}),
            "estimatedTokens": int(c.get("estimatedTokens", 80)),
            "protected": bool(c.get("protected", False)),
            "exact": bool(c.get("exact", True)),
            "recoverable": bool(c.get("recoverable", True)),
            "resolver": c.get("resolver", ""),
            "text": c.get("text", c.get("id", "")),
        })
    return candidates, "architect-federation-stub"
