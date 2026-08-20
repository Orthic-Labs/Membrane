"""Adapt CLI — direct transcript Taste v2 mining & reviewed-manifest apply."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from adapt import authority  # noqa: E402
from adapt import core_compiler  # noqa: E402
from adapt import manifest  # noqa: E402
from adapt import adapt_llm  # noqa: E402
from adapt import preference_record  # noqa: E402
from adapt import run_journal  # noqa: E402
from adapt import taste_apply  # noqa: E402
from adapt import taste_runtime  # noqa: E402
from adapt import taste_v2  # noqa: E402
from adapt import taste_v2_pipeline as pipeline  # noqa: E402
from adapt import transcript_sources  # noqa: E402
from adapt import workspace_runtime  # noqa: E402
from continuity.transcript import TranscriptUnavailable  # noqa: E402


def _candidate_records(sources, refs, authority_manifest: dict,
                       installation_id: str, *, llm_lane: str | None = None,
                       llm=None) -> tuple[list[dict], list[dict], list[dict], list[dict]]:
    """Build immutable pending/rejected records from span-preserving candidates."""
    by_rule: dict[str, list] = {}
    quarantined: list[dict] = []
    provenance_receipts: list[dict] = []
    llm_failures: list[dict] = []
    ref_by_path = {row["path"]: row for row in refs}
    for source in sources:
        try:
            provenance_receipt = {"source_id": pipeline.source_id(source, installation_id)}
            candidates = pipeline.extract_source(
                source, provenance_receipt=provenance_receipt,
                llm_lane=llm_lane, llm=llm,
            )
            provenance_receipts.append(provenance_receipt)
        except TranscriptUnavailable as exc:
            quarantined.append({"source_id": pipeline.source_id(source, installation_id),
                                "reason": f"transcript-unavailable:{exc.code}"})
            continue
        except Exception as exc:
            quarantined.append({"source_id": pipeline.source_id(source, installation_id),
                                "reason": f"parse-failed:{type(exc).__name__}"})
            continue
        for reason in (provenance_receipt.get("llm_proposer") or {}).get("failures", []):
            llm_failures.append({"source_id": pipeline.source_id(source, installation_id),
                                 "reason": reason})
        for candidate in candidates:
            admitted = taste_v2.admit_candidate(candidate)
            if admitted.lifecycleState != "active":
                quarantined.append({"source_id": pipeline.source_id(source, installation_id),
                                    "evidence_id": admitted.evidenceId,
                                    "reason": admitted.admissionReason})
                continue
            by_rule.setdefault(admitted.ruleId, []).append((source, admitted))
    records: list[dict] = []
    for rule_id, group in sorted(by_rule.items()):
        group.sort(key=lambda item: (item[1].sourceTranscriptId, item[1].sourceSequence,
                                     item[1].sourceByteStart))
        first = group[0][1]
        source_ids = sorted({pipeline.source_id(source, installation_id) for source, _candidate in group})
        contexts = [pipeline.evidence_context(candidate) for _source, candidate in group]
        evidence_ids = [{"evidence_id": manifest.derive_evidence_id(first.scope, candidate.evidenceText),
                         "source_session_id": pipeline.source_id(source, installation_id),
                         "excerpt": candidate.evidenceText}
                        for source, candidate in group]
        source_hashes = [{"session_id": pipeline.source_id(source, installation_id),
                          "sha256": ref_by_path[str(source.path)]["source_sha256"].removeprefix("sha256:")}
                         for source, _candidate in group]
        action = {"action": "add", "name": rule_id, "category": first.category,
                  "rule": first.rule, "confidence": 1.0, "observations": len(group),
                  "record_type": first.recordType, "needs_review": True}
        record = preference_record.PreferenceRecord.from_synthesis(
            action, scope=first.scope, source_ids=source_ids, evidence_contexts=contexts,
        )
        candidate = preference_record.to_manifest_candidate(
            record, evidence_excerpt=first.evidenceText, status="pending", operation="add"
        )
        candidate["source_file_hashes"] = source_hashes
        candidate["evidence_ids"] = evidence_ids
        candidate["authority_manifest_sha256"] = authority_manifest["manifest_sha256"]
        candidate["payload_sha256"] = manifest.payload_sha256(candidate)
        records.append(candidate)
    return records, quarantined, provenance_receipts, llm_failures


def _mine(args: argparse.Namespace) -> int:
    if args.apply:
        print("error: mined output cannot apply; review then use --apply-from-manifest", file=sys.stderr)
        return 2
    try:
        installation_id, canonical_rules = taste_runtime.multiwriter_context(
            manifest_body={}, required=True,
        )
    except taste_runtime.CrossMachineAdaptError as exc:
        print(f"error: mining requires multiwriter binding: {exc}", file=sys.stderr)
        return 2
    state_path = Path.home() / ".claude" / "adapt" / "taste-v2-state.json"
    state = pipeline.load_state(state_path)
    discovered = pipeline.discover()
    learn_cap = 3 if args.smoke else args.limit
    selected, typed_quarantine = transcript_sources.select_sources(
        discovered, learned=state.get("learned", {}), limit=learn_cap,
        installation_id=installation_id,
    )
    sources = pipeline.pending_sources(selected, state, limit=learn_cap,
                                       before_mtime=args.before_mtime, newest=args.smoke,
                                       installation_id=installation_id)
    quarantine = pipeline.quarantine_sources(typed_quarantine, installation_id)
    if not sources:
        print("adapt: no new direct transcript sources")
        return 0
    llm_lane = None if args.deterministic_only else args.lane
    if llm_lane and llm_lane != "local" and not args.allow_external_lane:
        print("error: external LLM lane requires --allow-external-lane", file=sys.stderr)
        return 2
    if llm_lane and not adapt_llm.lane_available(llm_lane):
        print(f"error: LLM proposer lane unavailable: {llm_lane}", file=sys.stderr)
        return 2
    refs = pipeline.source_refs(sources, installation_id)
    journal = run_journal.RunJournal()
    batch_id = run_journal.new_batch_id()
    replay = journal.pending_batch() if args.resume else None
    if replay:
        discovered = journal.cached_payload(replay["batch_id"], "discovered") or {}
        mismatch = pipeline.resume_mismatch_reason(discovered, refs)
        if mismatch:
            if args.restart_stale:
                journal.record(replay["batch_id"], "abandoned", reason="source_or_parser_stale")
            else:
                print(f"error: refusing unsafe resume: {mismatch}", file=sys.stderr)
                return 2
        else:
            batch_id = replay["batch_id"]
    if not replay or batch_id != replay["batch_id"]:
        journal.record(batch_id, "discovered", sessions=[row["source_id"] for row in refs],
                       source_refs=refs, extraction_contract=pipeline.extraction_contract(),
                       quarantined_sources=quarantine)
    authority_manifest = authority.build_manifest(workspace_runtime.workspace_root())
    records, extraction_quarantine, provenance_receipts, llm_failures = _candidate_records(
        sources, refs, authority_manifest, installation_id, llm_lane=llm_lane,
    )
    journal.record(batch_id, "extracted", source_parser_digests=sorted({
        context["sourceParserDigest"] for record in records
        for context in record["evidenceContexts"]
    }), transcript_provenance=provenance_receipts)
    if llm_failures:
        journal.record(batch_id, "abandoned", reason="llm_proposer_failed",
                       failures=llm_failures)
        print(f"error: LLM proposer failed for {len(llm_failures)} source batch(es)",
              file=sys.stderr)
        return 2
    journal.record(batch_id, "admitted", candidates=len(records),
                   quarantined=quarantine + extraction_quarantine)
    if args.manifest:
        body = {"schema_version": preference_record.DIRECT_MANIFEST_SCHEMA_VERSION, "batch_id": batch_id,
                "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "generator": "adapt.py --manifest direct-transcript-v2",
                "installation_id": installation_id,
                "canonical_pool_sha256": taste_runtime.cross_machine.canonical_pool_sha256(canonical_rules),
                "authority_manifest": authority_manifest,
                "source_session_ids": [row["source_id"] for row in refs], "source_refs": refs, "records": records}
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        temporary_path: Path | None = None
        try:
            with tempfile.NamedTemporaryFile(
                "w", encoding="utf-8", dir=args.manifest.parent,
                prefix=f".{args.manifest.name}.", suffix=".tmp", delete=False,
            ) as handle:
                temporary_path = Path(handle.name)
                handle.write(json.dumps(body, indent=2, ensure_ascii=False))
            manifest.validate_schema(temporary_path)
            os.replace(temporary_path, args.manifest)
            temporary_path = None
        except (OSError, manifest.ManifestError) as exc:
            if temporary_path is not None:
                temporary_path.unlink(missing_ok=True)
            journal.record(batch_id, "abandoned", reason="manifest_validation_failed")
            print(f"error: manifest validation failed: {exc}", file=sys.stderr)
            return 2
        print(f"adapt: wrote {args.manifest} ({len(records)} pending; {len(quarantine) + len(extraction_quarantine)} quarantined)")
        return 0
    print(f"adapt: direct transcript dry run ({len(sources)} sources; {len(records)} pending; {len(quarantine) + len(extraction_quarantine)} quarantined)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--backfill", action="store_true")
    mode.add_argument("--incremental", action="store_true")
    mode.add_argument("--smoke", action="store_true")
    mode.add_argument("--apply-from-manifest", type=Path)
    mode.add_argument("--insights", metavar="TRANSCRIPT", nargs="+")
    mode.add_argument("--token-spend", metavar="TRANSCRIPT", nargs="+",
                      dest="token_spend",
                      help="report where tokens were spent or wasted in a transcript")
    mode.add_argument("--compile-core", type=Path)
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--before-mtime", type=float)
    ap.add_argument("--resume", action="store_true")
    ap.add_argument("--restart-stale", action="store_true")
    ap.add_argument("--manifest", type=Path)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--quiet", action="store_true")
    ap.add_argument("--spend", action="store_true",
                    help="with --insights: print the token-spend table instead of the JSON report")
    ap.add_argument("--json", action="store_true",
                    help="with --token-spend: emit JSON instead of the text table")
    ap.add_argument("--rates", type=Path, default=None,
                    help="with --token-spend: model rate table (per million tokens)")
    ap.add_argument("--lane", default="local")
    ap.add_argument("--allow-external-lane", action="store_true")
    ap.add_argument("--deterministic-only", action="store_true",
                    help="skip optional LLM recall proposer")
    args = ap.parse_args()
    if args.apply_from_manifest:
        return taste_apply.apply_from_manifest(args.apply_from_manifest)
    if args.compile_core:
        if not pipeline.scanner_available() or not adapt_llm.lane_available(args.lane):
            print("error: core compilation preflight failed", file=sys.stderr)
            return 2
        records = taste_runtime.load_json(taste_runtime.rules_path())
        result = core_compiler.compile_and_write(records, args.compile_core, lane=args.lane)
        print(f"adapt: compiled {len(result['rules'])} core rules ({result['estimated_tokens']} estimated tokens) -> {args.compile_core}")
        return 0
    if args.token_spend:
        from adapt.token_spend import cli_token_spend
        extra = (["--json"] if args.json else []) + (["--rates", str(args.rates)] if args.rates else [])
        return cli_token_spend([*args.token_spend, *extra])
    if args.insights:
        from adapt.insights import cli_insights
        return cli_insights([*args.insights,
                             *(["--out", str(args.out)] if args.out else []),
                             *(["--quiet"] if args.quiet else []),
                             *(["--spend"] if args.spend else [])])
    return _mine(args)


def _dispatch(argv: list[str] | None = None) -> int:
    from adapt import doctor as adapt_doctor
    args = list(sys.argv[1:] if argv is None else argv)
    if args and args[0] in {"doctor", "doc"}:
        return adapt_doctor.main(args[1:])
    if argv is None:
        return main()
    old = sys.argv
    try:
        sys.argv = [old[0], *args]
        return main()
    finally:
        sys.argv = old


if __name__ == "__main__":
    raise SystemExit(_dispatch())
