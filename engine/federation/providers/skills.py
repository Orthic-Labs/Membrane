"""Skills provider — the ninth Membrane producer.

Emits the engine-owned skill catalog (index only) as bounded, delivery-ready candidates, so a
session in any repo discovers the same portable snapshot as memories without filesystem context
bleed. Bodies are NOT emitted here; they resolve on selection via `crypt skill-read <name>`
(progressive disclosure). Each candidate carries the engine snapshot's audited `bodyHash`.

Git remains the authoring source, while prompt-time ranking consumes one DB snapshot and returns
its generation seal to the gateway. Kill-switch: `RIGHTCONTEXT_SKILLS_PROVIDER=0`.
"""
from __future__ import annotations

import importlib.util
import json
import os
import re
import urllib.request
from pathlib import Path
from typing import Any

# Ranking implementation remains shared with the authoring catalog; candidate data comes only from
# the authenticated engine snapshot below.
_WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
_CATALOG_DIR = _WORKSPACE_ROOT / "tools" / "skills" / "skills-catalog"

_OFF = {"0", "false", "off", "no", ""}
_GENERATION = re.compile(r"sha256:[0-9a-f]{64}\Z")
_BODY_HASH = re.compile(r"[0-9a-f]{64}\Z")
MAX_SNAPSHOT_BYTES = 1024 * 1024
MAX_SKILLS = 512


def _enabled() -> bool:
    # Default ON (delivery is provenance-sealed and the provider emits only the bounded index);
    # an operator kill-switch flips it off without a redeploy.
    return os.environ.get("RIGHTCONTEXT_SKILLS_PROVIDER", "1").strip().lower() not in _OFF


def _load(mod_name: str):
    spec = importlib.util.spec_from_file_location(mod_name, _CATALOG_DIR / f"{mod_name.split('_')[-1]}.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _snapshot_from_serve() -> dict[str, Any]:
    port = os.environ.get("CRYPT_PORT") or os.environ.get("WORKSPACE_MEMORY_PORT") or "47851"
    token = ""
    token_file = os.environ.get("CRYPT_API_TOKEN_FILE", "")
    try:
        if token_file and os.path.exists(token_file):
            with open(token_file, "r", encoding="utf-8") as handle:
                token = handle.read().strip()
    except OSError as exc:
        raise RuntimeError("skills snapshot authentication unavailable") from exc
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/skills-snapshot",
        data=b"{}",
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"},
    )
    try:
        with urllib.request.urlopen(request, timeout=2) as response:
            body = response.read(MAX_SNAPSHOT_BYTES + 1)
    except Exception as exc:
        raise RuntimeError("engine skills snapshot unavailable") from exc
    if len(body) > MAX_SNAPSHOT_BYTES:
        raise RuntimeError("engine skills snapshot exceeds byte cap")
    try:
        payload = json.loads(body.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise RuntimeError("engine skills snapshot is invalid") from exc
    if not isinstance(payload, dict) or payload.get("schemaVersion") != 1:
        raise RuntimeError("engine skills snapshot schema mismatch")
    return payload


def produce(
    repo_root: Path, task: str, scope_grant_id: str | None = None
) -> tuple[list[dict[str, Any]], str, list[dict[str, str]]]:
    """Rank one engine-owned skills snapshot and return its exact generation seal.

    Git remains the authoring source, but prompt-time selection consumes the DB snapshot that
    travels across machines. The gateway compares this returned seal with the central freshness
    verdict, so a concurrent ingest can only omit this lane, never mix generations.
    """
    del repo_root, scope_grant_id
    if not _enabled():
        return [], "", []
    snapshot = _snapshot_from_serve()
    generation = str(snapshot.get("generation") or "")
    if not _GENERATION.fullmatch(generation):
        raise RuntimeError("engine skills generation is invalid")
    ingest = _load("skills_ingest")
    provider = _load("skills_provider")
    rows = snapshot.get("skills")
    if not isinstance(rows, list) or len(rows) > MAX_SKILLS:
        raise RuntimeError("engine skills snapshot row cap exceeded")
    catalog_skills = []
    for row in rows:
        if not isinstance(row, dict):
            raise RuntimeError("engine skills snapshot row is invalid")
        name = str(row.get("name") or "")
        body_hash = str(row.get("bodyHash") or "")
        description = str(row.get("description") or "")[:200]
        if not name or not _BODY_HASH.fullmatch(body_hash):
            raise RuntimeError("engine skills snapshot provenance is invalid")
        catalog_skills.append(
            {
                "name": name,
                "description": description,
                "triggers": ingest._triggers(name, description),
                "scope": "skills",
                "trustClass": "workspace_tracked",
                "bodyHash": body_hash,
                "resources": [],
            }
        )
    catalog = {"catalogVersion": 1, "scope": "skills", "skills": catalog_skills}
    ranked = provider.produce(task, catalog, limit=5)

    out: list[dict[str, Any]] = []
    for c in ranked.get("candidates", []):
        name = c["name"]
        body_hash = c["bodyHash"]
        desc = str(c.get("text", ""))[:200]
        score = float(c.get("providerScore", 0.45))
        out.append({
            "id": f"skills:{name}",
            "layer": 7,
            "sourceKind": "skill",
            "sourceRef": f"tools/skills/{name}/SKILL.md",
            "sourceHash": body_hash,
            "trustClass": c.get("trustClass", "workspace_tracked"),
            "instructionPolicy": "data_only",
            "providerScore": score,
            "scoreComponents": {"skill_rank": score},
            "estimatedTokens": max(1, len(desc) // 4),
            "protected": False,
            "exact": False,
            "recoverable": True,
            "resolver": c["resolver"],  # "crypt skill-read <name>"
            "text": desc,
            # Carve-out fields (recall_planner._format_packet_block): a skill block is delivered into
            # model-visible context ONLY when verify_skill(name, text, bodyHash) matches the audited skill.
            "name": name,
            "bodyHash": body_hash,
        })
    return out, generation, []
