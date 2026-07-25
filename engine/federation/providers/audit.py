"""Audit provider — invokes the typed AuditFinding provider.

Falls through to the existing audit_provider.produce_candidate_set if
present; otherwise emits an empty list (with a loud provider warning from
the gateway, not here).
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from . import canonical_repository_id


def _load_trust_overrides(repo_root: Path) -> dict[str, str]:
    """Read per-finding trustClass overrides from a sidecar JSON file.

    The federation gateway honours an optional
    ``.audit/federation/trust_overrides.json`` map of
    ``finding_id -> trustClass``. This is the canonical mechanism for
    promoting a finding to ``remote_untrusted`` (the trustClass the
    planner uses to reject cross-root evidence when no scope grant
    covers it).
    """
    path = repo_root / ".audit" / "federation" / "trust_overrides.json"
    if not path.is_file():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def produce(repo_root: Path, task: str) -> tuple[list[dict], str]:
    audit_provider_path = (
        Path(__file__).resolve().parents[3] / "skills" / "audit" / "audit_provider.py"
    )
    if not audit_provider_path.exists():
        raise FileNotFoundError(f"audit_provider.py missing at {audit_provider_path}")
    sys.path.insert(0, str(audit_provider_path.parent))
    try:
        from audit_provider import AuditProvider, ProviderContext  # type: ignore
        # The real provider requires a ProviderContext (repo_root +
        # repository_id + scope). The repository_id must be the workspace scope
        # slug, not the absolute path: audit_store.derive_finding_id hashes it
        # into every finding ID and specifies "never a machine path", so a path
        # here would fork identity per machine and break supersession.
        repo_id = canonical_repository_id(repo_root)
        ctx = ProviderContext(repo_root=repo_root, repository_id=repo_id, scope=repo_id)
        provider = AuditProvider(ctx=ctx)
        ccs = provider.produce_candidate_set(task=task, scope=str(repo_root))
    except Exception:
        raise
    finally:
        try:
            sys.path.remove(str(audit_provider_path.parent))
        except ValueError:
            pass

    trust_overrides = _load_trust_overrides(repo_root)
    raw_candidates = ccs.get("candidates", []) if isinstance(ccs, dict) else []
    candidates = []
    for c in raw_candidates:
        fid = c.get("id")
        trust_class = trust_overrides.get(fid, c.get("trustClass", "agent_verified"))
        candidates.append({
            "id": fid,
            "layer": int(c.get("layer", 4)),
            "sourceKind": c.get("sourceKind", "audit_finding"),
            "sourceRef": c.get("sourceRef", ""),
            "sourceHash": c.get("sourceHash", "0" * 64),
            "trustClass": trust_class,
            "instructionPolicy": c.get("instructionPolicy", "data_only"),
            "providerScore": float(c.get("providerScore", 0.5)),
            "scoreComponents": c.get("scoreComponents", {}),
            "estimatedTokens": int(c.get("estimatedTokens", 80)),
            "protected": False,
            "exact": True,
            "recoverable": True,
            "resolver": c.get("resolver", ""),
            "text": c.get("text", c.get("id", "")),
        })
    return candidates, "audit-federation-stub"
