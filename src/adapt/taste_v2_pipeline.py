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

from adapt import workspace_runtime

sys.path.insert(0, str(workspace_runtime.workspace_root()))
from tools.skills.legion.lib.orthic_transcripts import parse_source_events
from adapt import preference_record
from adapt import adapt_llm
from adapt import taste_v2
from adapt import transcript_sources

STATE_KEY = "taste_v2"

EXTRACTION_CONTRACT = "taste-v2-direct-transcripts-1"

def source_hash(path: Path) -> str:
    return transcript_sources.source_hash(path)

def discover(home: Path | None = None):
    return transcript_sources.discover(home)

def source_id(source: transcript_sources.TranscriptSource,
              installation_id: str | None = None) -> str:
    if installation_id:
        from adapt import cross_machine
        return cross_machine.qualify_source_session(
            installation_id, source.spec.tool, source.local_source_key,
        )
    return f"{source.spec.host}:{source.session_id}:{source.path.name}"

def load_state(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {"learned": {}}

def save_state(path: Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, indent=2, sort_keys=True), encoding="utf-8")

def source_refs(sources: list[transcript_sources.TranscriptSource],
                installation_id: str | None = None) -> list[dict[str, Any]]:
    return [{"source_id": source_id(s, installation_id), "tool": s.spec.tool, "host": s.spec.host,
             "path": str(s.path), "mtime_ns": s.path.stat().st_mtime_ns,
             "source_sha256": source_hash(s.path)} for s in sources]

def pending_sources(sources: list[transcript_sources.TranscriptSource], state: dict,
                    *, limit: int | None = None, before_mtime: float | None = None,
                    newest: bool = False,
                    installation_id: str | None = None) -> list[transcript_sources.TranscriptSource]:
    """Apply incremental learned/mtime filter to sources already partitioned by ``select_sources``.

    Callers must pass the ``selected`` half of ``transcript_sources.select_sources``: every
    source here is supported, has populated ``metadata``, and is not active. ``limit`` here
    only orders and trims; the inspection/cap budget lives in ``select_sources``.
    """
    learned = state.get("learned", {})
    eligible: list[transcript_sources.TranscriptSource] = []
    for source in sources:
        if source.spec.host not in transcript_sources.SUPPORTED_HOSTS:
            continue
        if source.metadata is None or source.metadata.exclusion_reason:
            continue
        if before_mtime is not None and source.path.stat().st_mtime > before_mtime:
            continue
        learned_id = source_id(source, installation_id)
        if learned.get(learned_id) == source_hash(source.path):
            continue
        eligible.append(source)
    eligible.sort(key=lambda s: s.path.stat().st_mtime, reverse=newest)
    if limit is not None:
        eligible = eligible[:limit]
    return sorted(eligible, key=lambda s: s.path.stat().st_mtime)

def quarantine_sources(sources: list[transcript_sources.TranscriptSource],
                       installation_id: str | None = None) -> list[dict[str, str]]:
    """Project already-quarantined descriptors (post-``select_sources``) to journal rows."""
    rows: list[dict[str, str]] = []
    for source in sources:
        host = source.spec.host
        reason = source.metadata.exclusion_reason if source.metadata is not None else ""
        if host not in transcript_sources.SUPPORTED_HOSTS and not reason:
            reason = "unsupported-host"
        rows.append({"source_id": source_id(source, installation_id), "host": str(host), "reason": reason or "unsupported-host"})
    return rows

def _llm_proposals(
    events: list[dict[str, Any]],
    source: transcript_sources.TranscriptSource,
    *,
    scope: str,
    lane: str,
    llm=None,
) -> tuple[list[taste_v2.TasteCandidateV1], list[dict[str, Any]], list[str]]:
    """Run LLM as proposer; bind every output back to its exact user event."""
    turns = [
        (source.spec.tool, scope_for_event(source, events[index], scope),
         str(events[index]["text"]), str(events[index]["sessionId"]), index)
        for index in taste_v2.iter_proposer_indices(events)
    ]
    candidates: list[taste_v2.TasteCandidateV1] = []
    receipts: list[dict[str, Any]] = []
    failures: list[str] = []
    for batch_number, batch in enumerate(adapt_llm.build_batches(turns), 1):
        outcome = adapt_llm.extract_observations(batch, llm=llm, lane=lane)
        receipt = {"batch": batch_number, "outcome": outcome.outcome,
                   "proposals": len(outcome.actions), **outcome.provider_receipt()}
        if outcome.reason:
            receipt["reason"] = outcome.reason
        receipts.append(receipt)
        if not outcome.committable:
            failures.append(f"llm-{outcome.outcome}:{outcome.reason or 'no detail'}")
            continue
        for proposal in outcome.actions:
            prompt = proposal.get("prompt")
            if not isinstance(prompt, int) or not (1 <= prompt <= len(batch)):
                failures.append("llm-invalid-prompt-binding")
                continue
            turn = batch[prompt - 1]
            try:
                candidate = taste_v2.propose_candidate(
                    events, int(turn[4]), str(proposal.get("observation") or ""),
                    category=str(proposal.get("category") or ""), scope=str(turn[1]),
                    record_type=("operational_playbook"
                                 if proposal.get("durability") == "cross_task_correction"
                                 else "standing_preference"),
                )
            except taste_v2.TasteV2Error as exc:
                failures.append(f"llm-provenance-rejected:{exc}")
                continue
            if candidate is not None:
                candidates.append(candidate)
    return candidates, receipts, failures


def extract_source(source: transcript_sources.TranscriptSource, *, scope: str = "workspace",
                   provenance_receipt: dict[str, Any] | None = None,
                   llm_lane: str | None = None, llm=None) -> list[taste_v2.TasteCandidateV1]:
    if source.spec.host not in transcript_sources.SUPPORTED_HOSTS or source.metadata.exclusion_reason:
        return []
    raw_events = parse_source_events(source.path, host=source.spec.host)
    events, receipt = transcript_sources.canonicalize_events(
        ({**event, "threadSource": source.metadata.thread_source} for event in raw_events)
    )
    if provenance_receipt is not None:
        provenance_receipt.update(receipt.as_dict())
    candidates = []
    for index in taste_v2.iter_candidate_indices(events):
        candidate = taste_v2.extract_candidate(
            events, index, scope=scope_for_event(source, events[index], scope),
        )
        if candidate is not None:
            candidates.append(candidate)
    if llm_lane:
        proposed, llm_receipts, llm_failures = _llm_proposals(
            events, source, scope=scope, lane=llm_lane, llm=llm,
        )
        candidates.extend(proposed)
        if provenance_receipt is not None:
            provenance_receipt["llm_proposer"] = {
                "lane": llm_lane, "batches": llm_receipts, "failures": llm_failures,
            }
    # Canonical record identity is preference identity, never a span-local alias.
    deduplicated = {}
    for candidate in candidates:
        identity = preference_record.derive_id(candidate.scope, candidate.category, candidate.rule)
        key = (identity, candidate.sourceEventId)
        deduplicated[key] = __import__('dataclasses').replace(candidate, ruleId=identity)
    return list(deduplicated.values())

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
        "evidenceId": candidate.evidenceId,
        "evidenceText": candidate.evidenceText,
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
    provenance_source = inspect.getsource(transcript_sources.canonicalize_events).encode("utf-8")
    proposer_source = (inspect.getsource(_llm_proposals) + adapt_llm.EXTRACT_SYSTEM).encode("utf-8")
    return {"name": EXTRACTION_CONTRACT,
            "parser_sha256": hashlib.sha256(parser_source).hexdigest(),
            "extractor_sha256": hashlib.sha256(extractor_source).hexdigest(),
            "provenance_sha256": hashlib.sha256(provenance_source).hexdigest(),
            "proposer_sha256": hashlib.sha256(proposer_source).hexdigest()}

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
    with tempfile.TemporaryDirectory(prefix="adapt-v2-scan-") as directory:
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
