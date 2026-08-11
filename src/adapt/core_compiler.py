"""Compile accepted root preferences into Adapt's bounded delivery core."""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
try:
    from adapt import workspace_runtime
    TOOLS_LIB = workspace_runtime.workspace_root() / "tools" / "lib"
except Exception:
    # Nested historical layout: tools/pipelines/memory/adapt → parents[2] == tools
    TOOLS_LIB = HERE.parents[2] / "lib"
if str(TOOLS_LIB) not in sys.path:
    sys.path.insert(0, str(TOOLS_LIB))

from adapt import adapt_llm
from adapt import adapt_sessions
from memory import adapt_core


SYSTEM = """Compile durable agent preferences into a compact always-loaded core.
Input contains root-scoped standing preferences only. Deduplicate semantic overlaps. Keep only
broad defaults useful across unrelated engineering tasks and safe to inject globally. Drop
product-, model-, provider-, tool-, format-, database-, UI-, and one-off workflow-specific rules;
those remain retrievable. Do not invent policy. Return ONLY JSON:
{"rules":[{"text":"imperative, <=18 words","source_ids":["..."]}]}
Use 6-14 rules and cite only supplied source IDs."""


def _sources(records) -> list[dict]:
    rows = records.values() if isinstance(records, dict) else records
    out = []
    for row in rows:
        source_id = row.get("id") or row.get("name")
        record_type = row.get("record_type")
        legacy_adapt_preference = (
            record_type == "unclassified"
            and isinstance(source_id, str)
            and source_id.startswith("adapt-")
        )
        if (
            source_id
            and (record_type == "standing_preference" or legacy_adapt_preference)
            and row.get("scope") == "D--Claude"
            and row.get("status", "accepted") == "accepted"
        ):
            out.append({"source_id": source_id, "rule": row["rule"]})
    return sorted(out, key=lambda row: row["source_id"])


def _parse_response(text: str) -> dict:
    cleaned = (text or "").strip()
    if cleaned.startswith("```"):
        cleaned = cleaned.split("\n", 1)[1].rsplit("```", 1)[0].strip()
    payload = json.loads(cleaned)
    return payload if isinstance(payload, dict) else {"rules": payload}


def compile_and_write(records, out: Path, *, lane: str = "minimax", call=None) -> dict:
    sources = _sources(records)
    if len(sources) < adapt_core.MIN_RULES:
        raise ValueError("not enough accepted root standing preferences to compile a core")
    request = json.dumps({"preferences": sources}, ensure_ascii=False)
    if not adapt_sessions.scan_batch_for_secrets_str(request):
        raise RuntimeError("secret scanner blocked core compiler input")
    caller = call or adapt_llm.call_lane_response
    response = caller(
        SYSTEM, request, lane=lane, max_tokens=3000, attempts=2,
        thinking="adaptive", temperature=0.1,
    )
    raw = response.get("text", "") if isinstance(response, dict) else str(response or "")
    if not adapt_sessions.scan_batch_for_secrets_str(raw):
        raise RuntimeError("secret scanner blocked core compiler output")
    parsed = _parse_response(raw)
    result = adapt_core.seal_rules(
        parsed.get("rules"), {row["source_id"] for row in sources}
    )
    result["input_source_count"] = len(sources)
    result["provider"] = {
        key: response.get(key) for key in ("model", "stop_reason", "stop_sequence", "usage")
        if isinstance(response, dict)
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(prefix=f".{out.name}.", suffix=".tmp", dir=out.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_name, out)
    finally:
        try:
            os.unlink(temp_name)
        except FileNotFoundError:
            pass
    return result
