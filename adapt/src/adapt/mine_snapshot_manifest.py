#!/usr/bin/env python3
"""Mine one frozen open-transcript corpus into a review-only Adapt manifest."""
from __future__ import annotations

import argparse
import concurrent.futures
import datetime as dt
import hashlib
import json
import os
import tempfile
from pathlib import Path

from adapt import authority
from adapt import adapt_llm
from adapt import cli
from adapt import manifest
from adapt import preference_record
from adapt import run_journal
from adapt import taste_runtime
from adapt import taste_v2
from adapt import taste_v2_pipeline as pipeline
from adapt import transcript_snapshots
from adapt import transcript_sources
from adapt import workspace_runtime
from continuity.transcript import parse_source_events


_TASTE_CUE = __import__("re").compile(
    r"(?i)\b(always|never|from now on|going forward|do not|don't|prefer|must|"
    r"the rule|standing rule|why did you|why are you|stop doing|stop using|wrong|incorrect|"
    r"agree|disagree|approve)\b"
)

_SOURCE_TURN_CHARS = 2_000
_PRIOR_CONTEXT_CHARS = 2_000


def _taste_excerpt(text: str, limit: int = 1400) -> str:
    """Keep directive neighborhoods while preserving unmodified source wording."""
    value = " ".join(str(text or "").split())
    matches = list(_TASTE_CUE.finditer(value))
    if not matches:
        return value[:limit]
    pieces = []
    for match in matches[:4]:
        start = max(0, match.start() - 220)
        end = min(len(value), match.end() + 420)
        piece = value[start:end]
        if piece not in pieces:
            pieces.append(piece)
    return " … ".join(pieces)[:limit]


def _mining_indices(events: list[dict]) -> list[int]:
    """Cover every policy-eligible external-user turn exactly once."""
    deterministic = list(taste_v2.iter_candidate_indices(events))
    recall = list(taste_v2.iter_proposer_indices(events))
    return sorted(set((*deterministic, *recall)))


def _contextual_turn(events: list[dict], index: int) -> str:
    """Give extractor correction context without granting context authority."""
    prior: list[str] = []
    remaining = _PRIOR_CONTEXT_CHARS
    for event in reversed(events[max(0, index - 6):index]):
        provenance = transcript_sources.event_provenance(event)
        if provenance not in {"external_user", "assistant"}:
            continue
        text = " ".join(str(event.get("text") or "").split())
        if not text:
            continue
        excerpt = text[:min(700, remaining)]
        prior.append(f"{provenance}: {excerpt}")
        remaining -= len(excerpt)
        if remaining <= 0 or len(prior) == 3:
            break
    prior.reverse()
    source = _taste_excerpt(
        str(events[index].get("text") or ""), limit=_SOURCE_TURN_CHARS,
    )
    context = "\n".join(prior) if prior else "(none)"
    return (
        "[NON-AUTHORITATIVE PRIOR CONTEXT]\n"
        f"{context}\n"
        "[AUTHORITATIVE SOURCE USER TURN]\n"
        f"{source}"
    )


def _llm_candidate_records(sources, refs, authority_manifest: dict, installation_id: str,
                           *, lane: str, workers: int = 5,
                           batch_char_budget: int = adapt_llm.BATCH_CHAR_BUDGET,
                           checkpoint_dir: Path | None = None):
    entries = []
    provenance = []
    canonical_user_turns = 0
    policy_excluded_user_turns = 0
    sources_with_mined_turns = 0
    for source in sources:
        raw = parse_source_events(source.path, host=source.spec.host)
        events, receipt = transcript_sources.canonicalize_events(
            ({**event, "threadSource": source.metadata.thread_source} for event in raw)
        )
        provenance.append({"source_id": pipeline.source_id(source, installation_id), **receipt.as_dict()})
        canonical_user_turns += receipt.eligible_user_turns
        indices = _mining_indices(events)
        sources_with_mined_turns += int(bool(indices))
        policy_excluded_user_turns += max(0, receipt.eligible_user_turns - len(indices))
        for index in indices:
            entries.append({
                "source": source, "events": events, "index": index,
                "scope": pipeline.scope_for_event(source, events[index], "workspace"),
                "text": _contextual_turn(events, index),
            })
    turns = [
        (entry["source"].spec.tool, entry["scope"], entry["text"], "entry", index)
        for index, entry in enumerate(entries)
    ]
    batches = adapt_llm.build_batches(turns, budget=batch_char_budget)

    checkpoint_contract = hashlib.sha256(
        (adapt_llm.EXTRACT_SYSTEM + "\0" + lane).encode("utf-8")
    ).hexdigest()

    def extract(number_and_batch):
        number, batch = number_and_batch
        input_sha256 = hashlib.sha256(
            json.dumps(batch, ensure_ascii=False, sort_keys=True).encode("utf-8")
        ).hexdigest()
        checkpoint = (
            checkpoint_dir / f"batch-{number:05d}.json" if checkpoint_dir else None
        )
        if checkpoint and checkpoint.is_file():
            try:
                cached = json.loads(checkpoint.read_text(encoding="utf-8"))
                if (
                    cached.get("input_sha256") == input_sha256
                    and cached.get("contract_sha256") == checkpoint_contract
                    and cached.get("outcome") in {"success", "valid_empty"}
                    and isinstance(cached.get("actions"), list)
                ):
                    from adapt.outcomes import BatchOutcome
                    return BatchOutcome(
                        cached["outcome"], cached["actions"], cached.get("reason", ""),
                        cached.get("usage"), cached.get("model"), cached.get("stop_reason"),
                    )
            except (OSError, ValueError, TypeError):
                pass
        outcome = adapt_llm.extract_observations(
            batch, lane=lane, allow_secret_turn_exclusion=True,
        )
        if checkpoint and outcome.committable:
            checkpoint.parent.mkdir(parents=True, exist_ok=True)
            body = {
                "input_sha256": input_sha256,
                "contract_sha256": checkpoint_contract,
                "outcome": outcome.outcome,
                "actions": outcome.actions,
                "reason": outcome.reason,
                "usage": outcome.usage,
                "model": outcome.model,
                "stop_reason": outcome.stop_reason,
            }
            temporary = checkpoint.with_suffix(".tmp")
            temporary.write_text(
                json.dumps(body, ensure_ascii=False, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            os.replace(temporary, checkpoint)
        return outcome

    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(5, workers))) as pool:
        outcomes = list(pool.map(extract, enumerate(batches, 1)))
    grouped: dict[str, list] = {}
    failures = []
    llm_receipts = []
    for number, (batch, outcome) in enumerate(zip(batches, outcomes), 1):
        llm_receipts.append({
            "batch": number, "outcome": outcome.outcome,
            "proposals": len(outcome.actions), **outcome.provider_receipt(),
        })
        if not outcome.committable:
            failures.append({"batch": number, "reason": outcome.reason or str(outcome.outcome)})
            continue
        for proposal in outcome.actions:
            prompt = proposal.get("prompt")
            if not isinstance(prompt, int) or not 1 <= prompt <= len(batch):
                failures.append({"batch": number, "reason": "invalid-prompt-binding"})
                continue
            entry = entries[int(batch[prompt - 1][4])]
            try:
                candidate = taste_v2.propose_candidate(
                    entry["events"], entry["index"], str(proposal.get("observation") or ""),
                    category=str(proposal.get("category") or ""), scope=entry["scope"],
                    record_type=("operational_playbook"
                                 if proposal.get("durability") == "cross_task_correction"
                                 else "standing_preference"),
                )
            except taste_v2.TasteV2Error:
                continue
            if candidate is None:
                continue
            admitted = taste_v2.admit_candidate(candidate)
            if admitted.lifecycleState == "active":
                identity = preference_record.derive_id(
                    admitted.scope, admitted.category, admitted.rule,
                )
                grouped.setdefault(identity, []).append((entry["source"], admitted))

    ref_by_path = {row["path"]: row for row in refs}
    records = []
    for _rule_id, group in sorted(grouped.items()):
        group.sort(key=lambda item: (item[1].sourceTranscriptId, item[1].sourceSequence,
                                     item[1].sourceByteStart))
        first = group[0][1]
        source_ids = sorted({pipeline.source_id(source, installation_id) for source, _ in group})
        contexts = [pipeline.evidence_context(candidate) for _, candidate in group]
        record_types = {candidate.recordType for _, candidate in group}
        action = {
            "action": "add", "category": first.category, "rule": first.rule,
            "confidence": 1.0, "observations": len(group),
            "record_type": ("standing_preference" if "standing_preference" in record_types
                            else first.recordType),
            "needs_review": True,
        }
        record = preference_record.PreferenceRecord.from_synthesis(
            action, scope=first.scope, source_ids=source_ids, evidence_contexts=contexts,
        )
        result = preference_record.to_manifest_candidate(
            record, evidence_excerpt=first.evidenceText, status="pending", operation="add",
        )
        result["source_file_hashes"] = [{
            "session_id": pipeline.source_id(source, installation_id),
            "sha256": ref_by_path[str(source.path)]["source_sha256"].removeprefix("sha256:"),
        } for source, _ in group]
        result["evidence_ids"] = [{
            "evidence_id": manifest.derive_evidence_id(first.scope, candidate.evidenceText),
            "source_session_id": pipeline.source_id(source, installation_id),
            "excerpt": candidate.evidenceText,
        } for source, candidate in group]
        result["authority_manifest_sha256"] = authority_manifest["manifest_sha256"]
        result["payload_sha256"] = manifest.payload_sha256(result)
        records.append(result)
    secret_policy_exclusions = sum(
        int((outcome.usage or {}).get("policyExcludedTurns", 0))
        for outcome in outcomes
    )
    coverage = {
        "complete": not failures and len(outcomes) == len(batches),
        "source_count": len(sources),
        "sources_with_mined_turns": sources_with_mined_turns,
        "canonical_user_turns": canonical_user_turns,
        "mined_user_turns": len(entries) - secret_policy_exclusions,
        "policy_excluded_user_turns": policy_excluded_user_turns + secret_policy_exclusions,
        "llm_batches": len(batches),
        "committable_batches": sum(outcome.committable for outcome in outcomes),
        "failed_batches": sum(not outcome.committable for outcome in outcomes),
        "batch_char_budget": batch_char_budget,
        "checkpointed_batches": sum(
            1 for number in range(1, len(batches) + 1)
            if checkpoint_dir and (checkpoint_dir / f"batch-{number:05d}.json").is_file()
        ),
        "selection_contract": "all-safe-external-user-turns-v1",
        "context_contract": "authoritative-source-with-prior-nonauthoritative-context-v1",
    }
    return records, provenance, llm_receipts, failures, coverage


def mine(snapshot_manifest: Path, output: Path, *, lane: str | None = None,
         workers: int = 5, shard_index: int = 0, shard_count: int = 1,
         batch_char_budget: int = 120_000) -> dict:
    if shard_count < 1 or not 0 <= shard_index < shard_count:
        raise ValueError("shard index/count are invalid")
    installation_id, canonical_rules = taste_runtime.multiwriter_context(
        manifest_body={}, required=True,
    )
    corpus_sources = transcript_snapshots.load_frozen_sources(snapshot_manifest)
    sources = corpus_sources[shard_index::shard_count]
    refs = pipeline.source_refs(sources, installation_id)
    authority_manifest = authority.build_manifest(workspace_runtime.workspace_root())
    batch_id = run_journal.new_batch_id()
    journal = run_journal.RunJournal()
    journal.record(
        batch_id, "discovered", sessions=[row["source_id"] for row in refs],
        source_refs=refs, extraction_contract=pipeline.extraction_contract(),
        frozen_snapshot_manifest_sha256=hashlib.sha256(snapshot_manifest.read_bytes()).hexdigest(),
    )
    llm_receipts = []
    input_turns = 0
    llm_batches = 0
    extraction_coverage = None
    if lane:
        if not adapt_llm.lane_available(lane):
            raise RuntimeError(f"LLM lane unavailable: {lane}")
        records, receipts, llm_receipts, failures, extraction_coverage = _llm_candidate_records(
            sources, refs, authority_manifest, installation_id, lane=lane, workers=workers,
            batch_char_budget=batch_char_budget,
            checkpoint_dir=output.parent / f"{output.stem}-calls",
        )
        input_turns = extraction_coverage["mined_user_turns"]
        llm_batches = extraction_coverage["llm_batches"]
        quarantined = []
    else:
        records, quarantined, receipts, failures = cli._candidate_records(
            sources, refs, authority_manifest, installation_id,
        )
    journal.record(
        batch_id, "extracted",
        source_parser_digests=sorted({
            context["sourceParserDigest"]
            for record in records for context in record["evidenceContexts"]
        }),
        transcript_provenance=receipts, llm_proposer=llm_receipts,
    )
    if failures:
        journal.record(batch_id, "abandoned", reason="extractor_failed", failures=failures)
        detail = "; ".join(
            f"batch {row.get('batch')}: {row.get('reason', 'unknown')}"
            for row in failures[:5]
        )
        raise RuntimeError(
            f"snapshot extraction failed: {len(failures)} ({detail})"
        )
    journal.record(batch_id, "admitted", candidates=len(records), quarantined=quarantined)
    snapshot_digest = hashlib.sha256(snapshot_manifest.read_bytes()).hexdigest()
    body = {
        "schema_version": preference_record.DIRECT_MANIFEST_SCHEMA_VERSION,
        "batch_id": batch_id,
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "generator": (
            f"adapt-frozen-open-transcripts-v2:{snapshot_digest}:shard-{shard_index + 1}-of-{shard_count}" if lane else
            f"adapt-frozen-open-transcripts-v1:{snapshot_digest}"
        ),
        "installation_id": installation_id,
        "canonical_pool_sha256": taste_runtime.cross_machine.canonical_pool_sha256(canonical_rules),
        "authority_manifest": authority_manifest,
        "source_session_ids": [row["source_id"] for row in refs],
        "source_refs": refs,
        "records": records,
    }
    if extraction_coverage is not None:
        body["extraction_coverage"] = extraction_coverage
        body["extraction_coverage"]["corpus_source_count"] = len(corpus_sources)
        body["extraction_coverage"]["shard_index"] = shard_index
        body["extraction_coverage"]["shard_count"] = shard_count
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", dir=output.parent,
            prefix=f".{output.name}.", suffix=".tmp", delete=False,
        ) as handle:
            temporary = Path(handle.name)
            json.dump(body, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
        manifest.validate_schema(temporary)
        os.replace(temporary, output)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return {
        "batch_id": batch_id,
        "sources": len(sources),
        "pending": len(records),
        "quarantined": len(quarantined),
        "llm_input_turns": input_turns,
        "llm_batches": llm_batches,
        "shard_index": shard_index,
        "shard_count": shard_count,
        "manifest": str(output),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("snapshot_manifest", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--lane", choices=("pi", "opencode", "local", "minimax"))
    parser.add_argument("--workers", type=int, default=5)
    parser.add_argument("--batch-char-budget", type=int, default=120_000)
    parser.add_argument("--shard-index", type=int, default=0)
    parser.add_argument("--shard-count", type=int, default=1)
    args = parser.parse_args()
    print(json.dumps(mine(
        args.snapshot_manifest, args.out, lane=args.lane, workers=args.workers,
        shard_index=args.shard_index, shard_count=args.shard_count,
        batch_char_budget=args.batch_char_budget,
    ), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
