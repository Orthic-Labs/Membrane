"""Taste apply — reviewed manifest → MemRight (zero LLM calls)."""
from __future__ import annotations

import datetime as dt
import json
import os
import sqlite3
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import adapt_sessions as ts  # noqa: E402
try:
    import run_journal  # noqa: E402
except ImportError:
    run_journal = None
import preference_record  # noqa: E402
import manifest  # noqa: E402
import authority  # noqa: E402
import rollback  # noqa: E402
import cross_machine  # noqa: E402
import adapt_persistence  # noqa: E402
import taste  # noqa: E402


def _host_module():
    if "adapt_cli" in sys.modules:
        return sys.modules["adapt_cli"]
    return sys.modules.get("adapt")


def _host_attr(name: str, default):
    host = _host_module()
    if host is not None and hasattr(host, name):
        return getattr(host, name)
    return default

def _preflight_apply_manifest() -> bool:
    """Minimal preflight for `--apply-from-manifest`: memright + scanner only.

    The LLM lane is irrelevant — no provider calls happen on apply.
    """
    if not ts.scanner_available():
        print("error: scanner (detect-secrets/gitleaks) unavailable; "
              "refusing manifest apply", file=sys.stderr)
        return False
    if not _host_attr("_run_memright", taste._run_memright)(["--help"]):
        print("error: memright shim unavailable; refusing manifest apply",
              file=sys.stderr)
        return False
    return True


def _create_apply_safepoint(manifest_body: dict) -> Path:
    """Create the mandatory pre-write Gate 4 safe-point."""
    db_override = os.environ.get("ADAPT_SAFEPOINT_DB_OVERRIDE")
    db_path = Path(db_override) if db_override else rollback._discover_db_path(manifest_body)
    if not db_path or not db_path.exists():
        raise RuntimeError(f"MemRight DB unavailable for safe-point: {db_path}")
    out_override = os.environ.get("ADAPT_SAFEPOINT_DIR_OVERRIDE")
    out_path = None
    if out_override:
        out_path = Path(out_override) / f"{manifest_body['batch_id']}.json"
    return rollback.create_safe_point(
        manifest_body,
        db_path,
        state_path=ts.STATE_FILE,
        rules_path=taste._rules_path(),
        core_path=ts.STATE_DIR / "core.json",
        out_path=out_path,
    )


def apply_from_manifest(manifest_path: Path) -> int:
    """Apply a reviewed manifest. Zero LLM calls; atomic write across accepted records.

    Gate 1a contract:
      - manifest schema-valid + payload_sha256-matches content
      - batch_id must match a journal discovered entry with the same sessions
      - only ``accepted`` records are written
      - state advances only after every put succeeds
      - on any put failure, partial writes are rolled back via memright delete
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

    jrn = run_journal.RunJournal()
    discovered = jrn.cached_payload(batch_id, "discovered")
    if not discovered or "sessions" not in discovered:
        print(f"error: no journal discovered entry for batch_id={batch_id}; "
              f"regenerate the manifest via --manifest before applying",
              file=sys.stderr)
        return 2
    j_sessions = sorted(discovered["sessions"])
    m_sessions = sorted(m["source_session_ids"])
    session_refs = discovered.get("session_refs") or [
        {"session_id": sid, "tool": "claude-code", "path_stem": sid, "mtime": 0}
        for sid in j_sessions
    ]
    if sorted(ref.get("source_key") or ref.get("session_id") for ref in session_refs) != j_sessions:
        print("error: journal session_refs mismatch discovered sessions", file=sys.stderr)
        return 2

    multiwriter = "installation_id" in m or "canonical_pool_sha256" in m
    multiwriter_context = None
    try:
        if multiwriter:
            multiwriter_context = _host_attr("_multiwriter_context", taste._multiwriter_context)(
                manifest_body=m, required=True
            )
            assert multiwriter_context is not None
            installation_id, canonical_rules = multiwriter_context
            expected_sessions = sorted(
                taste._qualified_session_sources(session_refs, installation_id)
            )
            cross_machine.validate_multiwriter_binding(
                m,
                installation_id=installation_id,
                canonical_rules=canonical_rules,
            )
        else:
            expected_sessions = j_sessions
    except (cross_machine.CrossMachineAdaptError, KeyError) as exc:
        print(f"error: refusing multiwriter manifest: {exc}", file=sys.stderr)
        return 2
    if expected_sessions != m_sessions:
        print("error: source_session_ids mismatch journal installation binding",
              file=sys.stderr)
        print(f"  journal:  {expected_sessions}", file=sys.stderr)
        print(f"  manifest: {m_sessions}", file=sys.stderr)
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
            authority_root=_host_attr("WORKSPACE_ROOT", taste.WORKSPACE_ROOT) if frozen_authority else None,
        )
        if not result.admitted:
            authority_rejected.append((rec["id"], result.reason))
    if authority_rejected:
        print("error: manifest contains authority-quarantined records:",
              file=sys.stderr)
        for record_id, reason in authority_rejected:
            print(f"  - {record_id}: {reason}", file=sys.stderr)
        return 2

    if not _host_attr("_preflight_apply_manifest", _preflight_apply_manifest)():
        return 2

    print(f"adapt: applying manifest {manifest_path}")
    print(f"  batch_id={batch_id}, sessions={len(j_sessions)}, "
          f"accepted={len(accepted)}, rejected={len(rejected)}")

    try:
        safe_point = _host_attr("_create_apply_safepoint", _create_apply_safepoint)(m)
    except (OSError, RuntimeError, ValueError, sqlite3.Error) as exc:
        print(f"error: refusing manifest apply; safe-point failed: {exc}",
              file=sys.stderr)
        taste._audit({"event": "manifest_safepoint_failed", "batch_id": batch_id,
                "error": str(exc)})
        jrn.record(batch_id, "applied", applied=0, ok=False,
                   failed=["safepoint"])
        return 2
    taste._audit({"event": "manifest_safepoint_created", "batch_id": batch_id,
            "path": str(safe_point)})
    print(f"  safepoint={safe_point}")

    # Parse the complete accepted set before any mutation.
    out_dir = ts.STATE_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    tmp_paths: list[Path] = []
    written: list[tuple[str, str]] = []
    failed: list[tuple[str, str]] = []
    prepared: list[preference_record.PreferenceRecord] = []

    # Cheap stub class: reconstruct enough path shape for each client's
    # canonical state key. Cline keys its messages filename without the suffix;
    # Grok Build and Roo/Cline key the transcript's parent directory.
    class _SessStub:
        def __init__(self, ref: dict) -> None:
            self.session_id = ref["session_id"]
            self.tool = ref["tool"]
            self.mtime = float(ref["mtime"])
            if self.tool == "cline":
                self.path = Path(f"{self.session_id}.messages.json")
            elif self.tool in {"grok-build", "roo-cline"}:
                self.path = Path(self.session_id) / ref["path_stem"]
            else:
                self.path = Path(ref["path_stem"])

    # Optional attribution and lifecycle metadata ride on the candidate but stay
    # outside manifest.candidate_payload's hash whitelist.
    apply_machine = preference_record.default_machine_id()
    for rec in accepted:
        try:
            retrieval_aliases = preference_record.normalize_retrieval_aliases(
                rec.get("retrieval_aliases", ()), rule=rec.get("rule", "")
            )
            if retrieval_aliases and not ts.scan_batch_for_secrets_str(
                json.dumps(list(retrieval_aliases), ensure_ascii=False)
            ):
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

    if not failed and multiwriter and prepared:
        assert multiwriter_context is not None
        installation_id, _canonical_rules = multiwriter_context
        try:
            adapt_persistence.persist_manifest_batch(
                prepared,
                manifest_batch_id=batch_id,
                installation_id=installation_id,
            )
        except adapt_persistence.AdaptPersistenceError as exc:
            failed.append((batch_id, str(exc)))
            taste._audit({"event": "manifest_batch_apply_failed", "batch_id": batch_id,
                    "error": str(exc)})
        else:
            written.extend((pr.id, pr.scope) for pr in prepared)
            for pr in prepared:
                taste._audit({"event": "manifest_applied", "id": pr.id,
                        "scope": pr.scope, "manifest": str(manifest_path),
                        "transport": "atomic_batch"})

    # Legacy manifests remain compatible until every installation has schema-v2
    # identity. Their compensating rollback path is intentionally not used by
    # multiwriter manifests, whose resident service transaction is all-or-nothing.
    if not failed and not multiwriter:
        for pr in prepared:
            body = preference_record.to_memright_content(pr)
            with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False,
                                             encoding="utf-8",
                                             dir=str(out_dir)) as tmp:
                tmp.write(body)
                tmp_path = tmp.name
            tmp_paths.append(Path(tmp_path))
            ok = _host_attr("_run_memright", taste._run_memright)(["put", pr.id, "--scope", pr.scope, "--file", tmp_path])
            if ok:
                written.append((pr.id, pr.scope))
                taste._audit({"event": "manifest_applied", "id": pr.id,
                        "scope": pr.scope, "manifest": str(manifest_path)})
            else:
                failed.append((pr.id, "memright put failed"))
                taste._audit({"event": "manifest_apply_failed", "id": pr.id,
                        "scope": pr.scope})
                break

    # Cleanup tmp files regardless of outcome.
    for tp in tmp_paths:
        try:
            Path(tp).unlink(missing_ok=True)
        except Exception:
            pass

    if failed:
        if not multiwriter:
            # Roll back partial legacy writes via the resident service.
            for w, scope in written:
                try:
                    qualified = f"{scope}/{w}"
                    if _host_attr("_run_memright", taste._run_memright)(["delete", qualified]):
                        taste._audit({"event": "manifest_rollback_deleted", "id": qualified})
                    else:
                        print(f"warn: failed to rollback {qualified}", file=sys.stderr)
                except OSError as exc:
                    print(f"warn: failed to rollback {scope}/{w}: {exc}", file=sys.stderr)
        print(f"error: {len(failed)} write(s) failed; rolled back "
              f"{len(written) if not multiwriter else 0} write(s); refusing state advance",
              file=sys.stderr)
        for name, why in failed:
            print(f"  - {name}: {why}", file=sys.stderr)
        jrn.record(batch_id, "applied", applied=0, ok=False,
                   failed=[n for n, _ in failed])
        return 1

    # Advance state over the manifest's bound session IDs only.
    state = ts.load_state()
    sess_stubs = [_SessStub(ref) for ref in session_refs]
    ts.mark_learned(state, sess_stubs)
    state["initialized_at"] = state.get("initialized_at") or \
        dt.datetime.now(dt.timezone.utc).isoformat()
    ts.save_state(state)

    # Mirror rules.json locally so adapt_digest stays usable.
    try:
        rules_obj: dict = {}
        rp = taste._rules_path()
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
            }
        rp.parent.mkdir(parents=True, exist_ok=True)
        rp.write_text(json.dumps(rules_obj, indent=2), encoding="utf-8")
        try:
            taste.write_digest(rules_obj, taste._digest_path())
        except Exception:
            pass
    except Exception as exc:
        print(f"warn: rules.json mirror write failed: {exc}", file=sys.stderr)

    jrn.record(batch_id, "applied", applied=len(accepted), ok=True,
               names=[name for name, _scope in written])
    jrn.record(batch_id, "committed", applied=len(accepted),
               sessions=j_sessions)
    print(f"adapt: applied {len(accepted)} manifest records; "
          f"sessions learned: {len(j_sessions)}")
    return 0
