"""Direct-transcript Taste v2 discovery/extraction boundary.

This module intentionally has no event-transport dependency: event metadata
cannot establish durable preference authority.
"""
from __future__ import annotations

import hashlib
import inspect
import json
import shutil
import subprocess
import tempfile
import sys
from dataclasses import asdict
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.lib.orthic_transcripts import parse_source_events
import preference_record
import taste_v2
import transcript_sources

STATE_KEY = "taste_v2"

EXTRACTION_CONTRACT = "taste-v2-direct-transcripts-1"

def source_hash(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()

def discover(home: Path | None = None):
    return transcript_sources.discover(home)

def source_id(source: transcript_sources.TranscriptSource) -> str:
    return f"{source.spec.host}:{source.session_id}:{source.path.name}"

def load_state(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {"learned": {}}

def save_state(path: Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, indent=2, sort_keys=True), encoding="utf-8")

def source_refs(sources: list[transcript_sources.TranscriptSource]) -> list[dict[str, Any]]:
    return [{"source_id": source_id(s), "tool": s.spec.tool, "host": s.spec.host,
             "path": str(s.path), "mtime_ns": s.path.stat().st_mtime_ns,
             "source_sha256": source_hash(s.path)} for s in sources]

def pending_sources(sources: list[transcript_sources.TranscriptSource], state: dict,
                    *, limit: int | None = None, before_mtime: float | None = None,
                    newest: bool = False) -> list[transcript_sources.TranscriptSource]:
    learned = state.get("learned", {})
    eligible = [s for s in sources if s.spec.host in transcript_sources.SUPPORTED_HOSTS and not s.metadata.exclusion_reason
                and (before_mtime is None or s.path.stat().st_mtime <= before_mtime)
                and learned.get(source_id(s)) != source_hash(s.path)]
    eligible.sort(key=lambda s: s.path.stat().st_mtime, reverse=newest)
    if limit is not None:
        eligible = eligible[:limit]
    return sorted(eligible, key=lambda s: s.path.stat().st_mtime)

def quarantine_sources(sources: list[transcript_sources.TranscriptSource]) -> list[dict[str, str]]:
    return [{"source_id": source_id(s), "host": str(s.spec.host), "reason": s.metadata.exclusion_reason or "unsupported-host"}
            for s in sources if s.spec.host not in transcript_sources.SUPPORTED_HOSTS or s.metadata.exclusion_reason]

def extract_source(source: transcript_sources.TranscriptSource, *, scope: str = "workspace") -> list[taste_v2.TasteCandidateV1]:
    if source.spec.host not in transcript_sources.SUPPORTED_HOSTS or source.metadata.exclusion_reason:
        return []
    events = parse_source_events(source.path, host=source.spec.host)
    candidates = taste_v2.extract_candidates(events, scope_for_event=lambda event: scope_for_event(source, event, scope))
    # Canonical record identity is preference identity, never a span-local alias.
    return [__import__('dataclasses').replace(c, ruleId=preference_record.derive_id(c.scope, c.category, c.rule)) for c in candidates]

def scope_for_event(source: transcript_sources.TranscriptSource, event: dict[str, Any], fallback: str = "workspace") -> str:
    """Map each event to a local cwd scope; never bake workspace names into production."""
    row = int(event.get("rowIndex") or 0)
    cwd = ""
    for at, value in source.metadata.cwd_by_row:
        if at <= row: cwd = value
        else: break
    if not cwd: return fallback
    root = Path(cwd)
    if not root.exists() or not root.is_dir(): return fallback
    lowered = cwd.lower()
    if any(token in lowered for token in ("health", "medical", "patient")): return fallback
    slug = "".join(char.lower() if char.isalnum() else "-" for char in cwd).strip("-")
    return slug or fallback

def evidence_context(candidate: taste_v2.TasteCandidateV1) -> dict[str, Any]:
    return {
        "sourceEventId": candidate.sourceEventId, "sourceSessionId": candidate.sourceSessionId,
        "sourceTranscriptId": candidate.sourceTranscriptId, "sourceParserDigest": candidate.sourceParserDigest,
        "sourceRowIndex": candidate.sourceRowIndex, "sourceSequence": candidate.sourceSequence,
        "sourceByteStart": candidate.sourceByteStart, "sourceByteEnd": candidate.sourceByteEnd,
        "sourceKind": candidate.sourceKind, "sourceRole": candidate.sourceRole,
        "sourceClassification": candidate.sourceClassification,
        "sourceFlags": dict(candidate.sourceFlags),
        "contextEvents": candidate.contextEvents,
    }

def evidence_id(candidate: taste_v2.TasteCandidateV1) -> str:
    data = "\0".join((candidate.sourceTranscriptId, candidate.sourceSessionId, candidate.sourceEventId, str(candidate.sourceByteStart), str(candidate.sourceByteEnd)))
    return "ev-" + hashlib.sha256(data.encode()).hexdigest()[:20]

def canonical_manifest_candidates(sources, *, scope: str = "workspace") -> list[dict[str, Any]]:
    grouped: dict[str, list[taste_v2.TasteCandidateV1]] = {}
    for source in sources:
        for candidate in extract_source(source, scope=scope):
            admitted = taste_v2.admit_candidate(candidate)
            if admitted.lifecycleState == "active": grouped.setdefault(admitted.ruleId, []).append(admitted)
    output = []
    for rule_id, group in sorted(grouped.items()):
        group.sort(key=lambda c: (c.sourceTranscriptId, c.sourceSequence, c.sourceByteStart))
        output.append({"id": rule_id, "rule": group[0].rule, "evidenceContexts": [evidence_context(c) for c in group], "evidence_ids": [evidence_id(c) for c in group]})
    return output

def extraction_contract() -> dict[str, str]:
    parser_source = inspect.getsource(parse_source_events).encode("utf-8")
    extractor_source = inspect.getsource(taste_v2.extract_candidates).encode("utf-8")
    return {"name": EXTRACTION_CONTRACT,
            "parser_sha256": hashlib.sha256(parser_source).hexdigest(),
            "extractor_sha256": hashlib.sha256(extractor_source).hexdigest()}

def resume_mismatch_reason(discovered: dict, refs: list[dict[str, Any]]) -> str | None:
    if discovered.get("extraction_contract") != extraction_contract():
        return "cached extractor contract does not match current extractor"
    if discovered.get("source_refs") != refs:
        return "cached source hash or identity does not match current discovery"
    return None

def scanner_available() -> bool:
    return bool(shutil.which("gitleaks") or shutil.which("detect-secrets"))

def scan_text(text: str) -> bool:
    """Fail closed if exact evidence aliases cannot pass local secret scanning."""
    if not scanner_available():
        return False
    with tempfile.TemporaryDirectory(prefix="morph-v2-scan-") as directory:
        path = Path(directory) / "payload.txt"
        path.write_text(text, encoding="utf-8")
        if shutil.which("gitleaks"):
            return subprocess.run(["gitleaks", "detect", "--no-git", "--redact", "--source", directory],
                                  capture_output=True, text=True, timeout=30).returncode == 0
        result = subprocess.run(["detect-secrets", "scan", str(path)], capture_output=True,
                                text=True, timeout=30)
        if result.returncode != 0:
            return False
        try:
            return not bool((json.loads(result.stdout or "{}").get("results") or {}).get(str(path)))
        except json.JSONDecodeError:
            return False
