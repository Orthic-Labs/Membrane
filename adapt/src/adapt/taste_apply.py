"""Taste apply — reviewed manifest → Membrane Cortex persistence (zero LLM calls)."""
from __future__ import annotations

import datetime as dt
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from adapt import taste_v2_pipeline as pipeline  # noqa: E402
from adapt import taste_runtime as runtime  # noqa: E402
try:
    from adapt import run_journal  # noqa: E402
except ImportError:
    run_journal = None
from adapt import preference_record  # noqa: E402
from adapt import manifest  # noqa: E402
from adapt import authority  # noqa: E402
from adapt import adapt_persistence  # noqa: E402
from adapt import workspace_runtime  # noqa: E402

WORKSPACE_ROOT = workspace_runtime.workspace_root()

# Kept as a module hook for test/runtime injection; implementation stays in the
# direct-runtime boundary, which delegates identity semantics to cross_machine.
_multiwriter_context = runtime.multiwriter_context

def _runtime_hook(name: str, default):
    host = sys.modules.get("adapt_cli") or sys.modules.get("adapt")
    # Do not invoke Adapt facade's lazy legacy imports while applying Taste v2.
    return host.__dict__.get(name, default) if host else default


def _extraction_coverage_error(manifest_body: dict, session_count: int) -> str | None:
    generator = str(manifest_body.get("generator") or "")
    if not generator.startswith("adapt-frozen-open-transcripts-v2:"):
        return None
    coverage = manifest_body.get("extraction_coverage")
    if not isinstance(coverage, dict):
        return "frozen transcript extraction coverage is missing"
    if coverage.get("complete") is not True:
        return "frozen transcript extraction is incomplete"
    if coverage.get("source_count") != session_count:
        return "frozen transcript source coverage does not match journal"
    corpus_source_count = coverage.get("corpus_source_count")
    shard_index = coverage.get("shard_index")
    shard_count = coverage.get("shard_count")
    if not all(isinstance(value, int) and not isinstance(value, bool)
               for value in (corpus_source_count, shard_index, shard_count)):
        return "frozen transcript shard coverage is invalid"
    if corpus_source_count < session_count or shard_count < 1 or not 0 <= shard_index < shard_count:
        return "frozen transcript shard coverage is invalid"
    canonical = coverage.get("canonical_user_turns")
    mined = coverage.get("mined_user_turns")
    excluded = coverage.get("policy_excluded_user_turns")
    if not all(isinstance(value, int) and not isinstance(value, bool) and value >= 0
               for value in (canonical, mined, excluded)):
        return "frozen transcript turn coverage is invalid"
    if mined + excluded != canonical:
        return "frozen transcript turn coverage is incomplete"
    if coverage.get("failed_batches") != 0:
        return "frozen transcript extraction contains failed batches"
    if coverage.get("committable_batches") != coverage.get("llm_batches"):
        return "frozen transcript extraction batch coverage is incomplete"
    return None

def apply_from_manifest(manifest_path: Path) -> int:
    """Apply a reviewed manifest. Zero LLM calls; resumable bounded writes.

    Gate 1a contract:
      - manifest schema-valid + payload_sha256-matches content
      - batch_id must match a journal discovered entry with the same sessions
      - only independently semantically validated ``accepted`` records are written
      - state advances only after the authenticated batch receipt is complete
      - each Cortex chunk is atomic and replayable; state advances after every chunk completes
    """
    try:
        m = manifest.apply_time_validate(manifest_path)
    except manifest.ManifestError as exc:
        print(f"error: manifest invalid: {exc}", file=sys.stderr)
        return 2

    if m.get("schema_version") != "1.3.0":
        print("error: only manifest schema 1.3.0 is supported", file=sys.stderr)
        return 2

    batch_id = m["batch_id"]
    if run_journal is None:
        print("error: run_journal module unavailable; refusing manifest apply",
              file=sys.stderr)
        return 2

    installation_id: str | None = None
    canonical_rules: dict | None = None
    # Binding is deliberately first: no discovered payload or network write is
    # consulted before installation/pool validation.
    try:
        installation_id, canonical_rules = _runtime_hook(
            "_multiwriter_context", _multiwriter_context
        )(manifest_body=m, required=True)
        runtime.validate_multiwriter_binding(
            m, installation_id=installation_id, canonical_rules=canonical_rules,
        )
    except runtime.CrossMachineAdaptError as exc:
        print(f"error: refusing manifest apply: {exc}", file=sys.stderr)
        return 2

    jrn = run_journal.RunJournal()
    discovered = jrn.cached_payload(batch_id, "discovered")
    if not discovered or "sessions" not in discovered:
        print(f"error: no journal discovered entry for batch_id={batch_id}; "
              f"regenerate the manifest via --manifest before applying",
              file=sys.stderr)
        return 2
    session_refs = discovered.get("source_refs")
    if not session_refs:
        print("error: journal source_refs mismatch manifest", file=sys.stderr)
        return 2
    try:
        j_sessions = runtime.qualify_session_sources(session_refs, installation_id)
    except (KeyError, ValueError, runtime.CrossMachineAdaptError) as exc:
        print(f"error: journal source_refs cannot be qualified: {exc}", file=sys.stderr)
        return 2
    qualified_refs = [{**ref, "source_id": source_id}
                      for ref, source_id in zip(session_refs, j_sessions)]
    if m.get("source_refs") != qualified_refs:
        print("error: journal source_refs mismatch manifest", file=sys.stderr)
        return 2
    if m["source_session_ids"] != j_sessions:
        print("error: source_session_ids mismatch journal source_refs", file=sys.stderr)
        return 2
    coverage_error = _extraction_coverage_error(m, len(j_sessions))
    if coverage_error:
        print(f"error: {coverage_error}; refusing state advance", file=sys.stderr)
        return 2

    accepted = manifest.accepted_records(m)
    rejected = manifest.rejected_records(m)
    frozen_authority = m.get("authority_manifest")
    authority_rejected = []
    for rec in accepted:
        result = authority.evaluate_rule(
            rec["rule"],
            scope=rec["scope"],
            declared_effect=rec.get("authority_effect"),
            authority_manifest=frozen_authority,
            authority_root=_runtime_hook("WORKSPACE_ROOT", WORKSPACE_ROOT) if frozen_authority else None,
        )
        if not result.admitted:
            authority_rejected.append((rec["id"], result.reason))
    if authority_rejected:
        print("error: manifest contains authority-quarantined records:",
              file=sys.stderr)
        for record_id, reason in authority_rejected:
            print(f"  - {record_id}: {reason}", file=sys.stderr)
        return 2

    print(f"adapt: applying manifest {manifest_path}")
    print(f"  batch_id={batch_id}, sessions={len(j_sessions)}, "
          f"accepted={len(accepted)}, rejected={len(rejected)}")

    # Parse the complete accepted set before any mutation.
    failed: list[tuple[str, str]] = []
    prepared: list[preference_record.PreferenceRecord] = []
    batch_receipt: dict | None = None

    # Optional attribution and lifecycle metadata ride on the candidate but stay
    # outside manifest.candidate_payload's hash whitelist.
    apply_machine = preference_record.default_machine_id()
    for rec in accepted:
        try:
            retrieval_aliases = preference_record.normalize_retrieval_aliases(
                rec.get("retrieval_aliases", ()), rule=rec.get("rule", "")
            )
            if retrieval_aliases and not pipeline.scan_text(json.dumps(list(retrieval_aliases), ensure_ascii=False)):
                raise ValueError("retrieval aliases failed privacy scanner")
            pr = preference_record.from_manifest_candidate(
                {**rec, "retrieval_aliases": retrieval_aliases},
                now=m["created_at"],
                machine=rec.get("machine") or apply_machine,
                machine_only=rec.get("machine_only"),
            )
        except (KeyError, ValueError) as exc:
            failed.append((rec["id"], f"contract-invalid: {exc}"))
            continue
        prepared.append(pr)

    if not failed:
        try:
            batch_receipt = _runtime_hook(
                "persist_manifest_batch", adapt_persistence.persist_manifest_batch
            )(
                prepared,
                manifest_batch_id=batch_id,
                installation_id=installation_id,
                semantic_validation=m["semantic_validation"],
                record_payload_sha256s={
                    rec["id"]: rec["payload_sha256"] for rec in accepted
                },
            )
            if batch_receipt.get("complete") is not True:
                raise adapt_persistence.AdaptPersistenceError("Cortex batch receipt is incomplete")
        except (OSError, RuntimeError, ValueError,
                adapt_persistence.AdaptPersistenceError) as exc:
            failed.append(("batch", str(exc)))

    if failed:
        print(f"error: {len(failed)} write(s) failed; rolled back "
              "atomically by Cortex; refusing state advance",
              file=sys.stderr)
        for name, why in failed:
            print(f"  - {name}: {why}", file=sys.stderr)
        jrn.record(batch_id, "applied", applied=0, ok=False,
                   failed=[n for n, _ in failed])
        return 1

    state = runtime.load_json(runtime.state_path(), {"learned": {}})
    learned_refs = zip(j_sessions, session_refs)
    for source_id, ref in learned_refs:
        if ref.get("source_sha256"):
            state.setdefault("learned", {})[source_id] = ref["source_sha256"]
    state["initialized_at"] = state.get("initialized_at") or dt.datetime.now(dt.timezone.utc).isoformat()
    runtime.write_json_atomic(runtime.state_path(), state)

    # Mirror rules.json locally so adapt_digest stays usable.
    try:
        rules_obj: dict = {}
        rp = runtime.rules_path()
        if rp.exists():
            rules_obj = json.loads(rp.read_text(encoding="utf-8"))
        # `prepared` (the resolved PreferenceRecords) carries the attributed
        # machine; `accepted` (raw manifest records) never does. Same order
        # and length here — this block only runs once `not failed`.
        for rec, pr in zip(accepted, prepared):
            rules_obj[rec["id"]] = {
                "name": rec["id"],
                "category": rec["category"],
                "rule": rec["rule"],
                "confidence": rec.get("confidence", 0.6),
                "observations": rec.get("evidence_count", 1),
                "scope": rec["scope"],
                "needs_review": rec.get("needs_review", False),
                "record_type": rec.get("record_type", "unclassified"),
                "authority_effect": rec.get("authority_effect", "neutral"),
                "retrieval_aliases": list(rec.get("retrieval_aliases", [])),
                "machine": pr.machine,
                "machine_only": pr.machine_only,
                "evidence_contexts": list(pr.evidence_contexts),
            }
        rp.parent.mkdir(parents=True, exist_ok=True)
        runtime.write_json_atomic(rp, rules_obj)
    except Exception as exc:
        print(f"warn: rules.json mirror write failed: {exc}", file=sys.stderr)

    applied_payload = {
        "applied": len(accepted),
        "ok": True,
        "names": (
            [record.id for record in prepared]
        ),
    }
    if batch_receipt is not None:
        applied_payload["receipt"] = batch_receipt
    jrn.record(batch_id, "applied", **applied_payload)
    jrn.record(batch_id, "committed", applied=len(accepted),
               sessions=j_sessions)
    print(f"adapt: applied {len(accepted)} manifest records; "
          f"sessions learned: {len(j_sessions)}")
    return 0
