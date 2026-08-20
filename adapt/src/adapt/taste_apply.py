"""Taste apply — reviewed manifest → Cortex (zero LLM calls)."""
from __future__ import annotations

import datetime as dt
import json
import os
import sqlite3
import sys
import tempfile
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
from adapt import rollback  # noqa: E402
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

def _preflight_apply_manifest() -> bool:
    """Minimal preflight for `--apply-from-manifest`: cortex + scanner only.

    The LLM lane is irrelevant — no provider calls happen on apply.
    """
    if not _runtime_hook("_scanner_available", runtime.scanner_available)():
        print("error: scanner (detect-secrets/gitleaks) unavailable; "
              "refusing manifest apply", file=sys.stderr)
        return False
    if not _runtime_hook("_run_cortex", runtime.run_cortex)(["--help"]):
        print("error: cortex shim unavailable; refusing manifest apply",
              file=sys.stderr)
        return False
    return True


def _create_apply_safepoint(manifest_body: dict) -> Path:
    """Create the mandatory pre-write Gate 4 safe-point."""
    db_override = os.environ.get("ADAPT_SAFEPOINT_DB_OVERRIDE")
    db_path = Path(db_override) if db_override else rollback._discover_db_path(manifest_body)
    if not db_path or not db_path.exists():
        raise RuntimeError(f"Cortex DB unavailable for safe-point: {db_path}")
    out_override = os.environ.get("ADAPT_SAFEPOINT_DIR_OVERRIDE")
    out_path = None
    if out_override:
        out_path = Path(out_override) / f"{manifest_body['batch_id']}.json"
    return rollback.create_safe_point(
        manifest_body,
        db_path,
        state_path=runtime.state_path(),
        rules_path=runtime.rules_path(),
        core_path=runtime.state_dir() / "core.json",
        out_path=out_path,
    )


def apply_from_manifest(manifest_path: Path) -> int:
    """Apply a reviewed manifest. Zero LLM calls; atomic write across accepted records.

    Gate 1a contract:
      - manifest schema-valid + payload_sha256-matches content
      - batch_id must match a journal discovered entry with the same sessions
      - only ``accepted`` records are written
      - state advances only after every put succeeds
      - on any put failure, partial writes are rolled back via cortex delete
        and state does NOT advance
    """
    try:
        m = manifest.load_and_validate(manifest_path)
    except manifest.ManifestError as exc:
        print(f"error: manifest invalid: {exc}", file=sys.stderr)
        return 2

    batch_id = m["batch_id"]
    if run_journal is None:
        print("error: run_journal module unavailable; refusing manifest apply",
              file=sys.stderr)
        return 2

    is_multiwriter = m.get("schema_version") == "1.3.0"
    installation_id: str | None = None
    canonical_rules: dict | None = None
    if is_multiwriter:
        # Binding is deliberately first: no discovered payload, safe-point, or
        # network write is consulted before installation/pool validation.
        try:
            installation_id, canonical_rules = _runtime_hook(
                "_multiwriter_context", _multiwriter_context
            )(
                manifest_body=m, required=True
            )
            runtime.validate_multiwriter_binding(
                m,
                installation_id=installation_id,
                canonical_rules=canonical_rules,
            )
        except runtime.CrossMachineAdaptError as exc:
            print(f"error: refusing multiwriter manifest apply: {exc}", file=sys.stderr)
            return 2

    jrn = run_journal.RunJournal()
    discovered = jrn.cached_payload(batch_id, "discovered")
    if not discovered or "sessions" not in discovered:
        print(f"error: no journal discovered entry for batch_id={batch_id}; "
              f"regenerate the manifest via --manifest before applying",
              file=sys.stderr)
        return 2
    session_refs = discovered.get("source_refs")
    if not is_multiwriter and not session_refs:
        session_refs = [{"source_id": value, "source_sha256": ""} for value in discovered.get("sessions", [])]
    if not session_refs:
        print("error: journal source_refs mismatch manifest", file=sys.stderr)
        return 2
    if is_multiwriter:
        try:
            j_sessions = runtime.qualify_session_sources(session_refs, installation_id)
        except (KeyError, ValueError, runtime.CrossMachineAdaptError) as exc:
            print(f"error: journal source_refs cannot be qualified: {exc}", file=sys.stderr)
            return 2
        qualified_refs = [
            {**ref, "source_id": source_id}
            for ref, source_id in zip(session_refs, j_sessions)
        ]
        if m.get("source_refs") != qualified_refs:
            print("error: journal source_refs mismatch manifest", file=sys.stderr)
            return 2
    else:
        j_sessions = [ref["source_id"] for ref in session_refs]
    if m["source_session_ids"] != j_sessions:
        print("error: source_session_ids mismatch journal source_refs", file=sys.stderr)
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

    # v1.3 persists through one authenticated API request, not Cortex CLI.
    if not is_multiwriter and not _preflight_apply_manifest():
        return 2

    print(f"adapt: applying manifest {manifest_path}")
    print(f"  batch_id={batch_id}, sessions={len(j_sessions)}, "
          f"accepted={len(accepted)}, rejected={len(rejected)}")

    # Parse the complete accepted set before any mutation.
    out_dir = runtime.state_dir()
    tmp_paths: list[Path] = []
    written: list[tuple[str, str]] = []
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

    if not failed and is_multiwriter:
        try:
            safe_point = _runtime_hook("_create_apply_safepoint", _create_apply_safepoint)(m)
            print(f"  safepoint={safe_point}")
            if prepared:
                batch_receipt = _runtime_hook(
                    "persist_manifest_batch", adapt_persistence.persist_manifest_batch
                )(
                    prepared,
                    manifest_batch_id=batch_id,
                    installation_id=installation_id,
                )
                if batch_receipt.get("complete") is not True:
                    raise adapt_persistence.AdaptPersistenceError("Cortex batch receipt is incomplete")
        except (OSError, RuntimeError, ValueError, sqlite3.Error,
                adapt_persistence.AdaptPersistenceError) as exc:
            failed.append(("batch", str(exc)))
    elif not failed and not is_multiwriter:
        try:
            safe_point = _runtime_hook("_create_apply_safepoint", _create_apply_safepoint)(m)
        except (OSError, RuntimeError, ValueError, sqlite3.Error) as exc:
            print(f"error: refusing manifest apply; safe-point failed: {exc}", file=sys.stderr)
            jrn.record(batch_id, "applied", applied=0, ok=False, failed=["safepoint"])
            return 2
        print(f"  safepoint={safe_point}")
        out_dir.mkdir(parents=True, exist_ok=True)
        for pr in prepared:
            body = preference_record.to_cortex_content(pr)
            with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False,
                                             encoding="utf-8",
                                             dir=str(out_dir)) as tmp:
                tmp.write(body)
                tmp_path = tmp.name
            tmp_paths.append(Path(tmp_path))
            ok = _runtime_hook("_run_cortex", runtime.run_cortex)(["put", pr.id, "--scope", pr.scope, "--file", tmp_path])
            if ok:
                written.append((pr.id, pr.scope))
            else:
                failed.append((pr.id, "cortex put failed"))
                break

    # Cleanup tmp files regardless of outcome.
    for tp in tmp_paths:
        try:
            Path(tp).unlink(missing_ok=True)
        except Exception:
            pass

    if failed:
        if not is_multiwriter:
            for w, scope in written:
                _runtime_hook("_run_cortex", runtime.run_cortex)(["delete", f"{scope}/{w}"])
        print(f"error: {len(failed)} write(s) failed; rolled back "
              f"{len(written)} write(s); refusing state advance",
              file=sys.stderr)
        for name, why in failed:
            print(f"  - {name}: {why}", file=sys.stderr)
        jrn.record(batch_id, "applied", applied=0, ok=False,
                   failed=[n for n, _ in failed])
        return 1

    state = runtime.load_json(runtime.state_path(), {"learned": {}})
    learned_refs = zip(j_sessions, session_refs) if is_multiwriter else (
        (ref["source_id"], ref) for ref in session_refs
    )
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
            if is_multiwriter else [name for name, _scope in written]
        ),
    }
    if is_multiwriter:
        if batch_receipt is not None:
            applied_payload["receipt"] = batch_receipt
    jrn.record(batch_id, "applied", **applied_payload)
    jrn.record(batch_id, "committed", applied=len(accepted),
               sessions=j_sessions)
    print(f"adapt: applied {len(accepted)} manifest records; "
          f"sessions learned: {len(j_sessions)}")
    return 0
