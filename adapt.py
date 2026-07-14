"""adapt — mine durable working preferences from local session transcripts into MemRight.

Usage (from workspace root):
  py -3.11 tools/pipelines/memory/adapt/adapt.py --smoke --lane local    # 3-session dry run
  py -3.11 tools/pipelines/memory/adapt/adapt.py --backfill --dry-run    # preview everything
  py -3.11 tools/pipelines/memory/adapt/adapt.py --backfill --apply --first-run-ok
  py -3.11 tools/pipelines/memory/adapt/adapt.py --incremental --apply --limit 20

Stages: discover -> extract (LLM) -> synthesize (LLM) -> apply (memright put + digest + audit).
State/rules/digest live in ~/.claude/adapt/ (machine-local, disposable).
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import adapt_llm  # noqa: E402
import adapt_sessions as ts  # noqa: E402
import outcomes  # noqa: E402
try:
    import run_journal  # noqa: E402
except ImportError:
    run_journal = None  # journal is optional; pipeline works without it
try:
    import admission  # noqa: E402
except ImportError:
    admission = None
import preference_record  # noqa: E402
import manifest  # noqa: E402
import authority  # noqa: E402
import core_compiler  # noqa: E402
import rollback  # noqa: E402

RULES_FILE = ts.STATE_DIR / "rules.json"
DIGEST_FILE = ts.STATE_DIR / "adapt-digest.md"
WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
MEMRIGHT_MUTATION_TIMEOUT_SECONDS = 150


def _rules_path() -> Path:
    """Late-bound rules.json path; monkeypatching ts.STATE_DIR works in tests."""
    return ts.STATE_DIR / "rules.json"


def _digest_path() -> Path:
    """Late-bound digest path; monkeypatching ts.STATE_DIR works in tests."""
    return ts.STATE_DIR / "adapt-digest.md"


def _audit_file() -> Path:
    """Compute audit target lazily, so tests can route writes via env override.

    Tests that exercise `_audit` should `os.environ["ADAPT_AUDIT_FILE_OVERRIDE"]`
    to a tmp_path before importing `adapt`. Operational runs have no override
    and write to the canonical `~/.claude/adapt/audit.jsonl`.
    """
    override = os.environ.get("ADAPT_AUDIT_FILE_OVERRIDE")
    if override:
        return Path(override)
    return ts.STATE_DIR / "audit.jsonl"


def _run_memright(args: list[str]) -> bool:
    """Invoke the installed memright shim (never touches tokens). Fail closed with the error."""
    bin_path = shutil.which("memright")
    if not bin_path:
        print("error: memright shim not on PATH; install via setup-workspace.py first",
              file=sys.stderr)
        return False
    try:
        res = subprocess.run(
            [bin_path, *args], capture_output=True, text=True,
            timeout=MEMRIGHT_MUTATION_TIMEOUT_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        print(f"error: memright shim failed: {exc}", file=sys.stderr)
        return False
    if res.returncode != 0:
        print(f"error: memright {args[0]} rc={res.returncode}: {res.stderr.strip()}",
              file=sys.stderr)
        return False
    return True


def preflight_apply(lane: str, allow_external: bool) -> bool:
    if not ts.scanner_available():
        print("error: adapt apply requires detect-secrets or gitleaks before transcript text can leave the machine",
              file=sys.stderr)
        return False
    if lane != "local" and not allow_external:
        print("error: external LLM lanes require --allow-external-lane", file=sys.stderr)
        return False
    if not adapt_llm.lane_available(lane):
        print(f"error: LLM lane unavailable: {lane}", file=sys.stderr)
        return False
    if not _run_memright(["--help"]):
        print("error: memright shim is unavailable; run setup-workspace.py first", file=sys.stderr)
        return False
    return True


def initialized(state: dict) -> bool:
    return bool(state.get("initialized_at"))


def rule_body(rule: dict, evidence: str, tool: str) -> str:
    today = dt.date.today().isoformat()
    aliases = preference_record.normalize_retrieval_aliases(
        rule.get("retrieval_aliases", ()), rule=rule.get("rule", "")
    )
    alias_line = f"**Trigger phrases:** {' | '.join(aliases)}\n" if aliases else ""
    return (
        f"**[adapt/{rule['category']}]** — {rule['rule']} "
        f"Confidence: {rule['confidence']:.2f} "
        f"(observations: {rule['observations']}, needs_review: {str(rule.get('needs_review', False)).lower()}, "
        f"updated {today})\n"
        f"**Why:** mined from {tool or 'session'} prompts; e.g. \"{evidence}\"\n"
        f"**Record:** type={rule.get('record_type', 'unclassified')}, "
        f"authority_effect={rule.get('authority_effect', 'neutral')}\n"
        f"{alias_line}"
        f"**How to apply:** {preference_record.application_guidance(rule.get('record_type', 'unclassified'))}\n"
    )


def _safe_retrieval_aliases(rule: str, observations: list[dict]) -> tuple[str, ...]:
    """Return bounded evidence cues only after the full alias payload scans clean."""
    aliases = preference_record.normalize_retrieval_aliases(
        [item.get("evidence", "") for item in observations], rule=rule
    )
    if not aliases:
        return ()
    payload = json.dumps(list(aliases), ensure_ascii=False)
    if not ts.scan_batch_for_secrets_str(payload):
        _audit({"event": "retrieval_aliases_scanner_blocked", "count": len(aliases)})
        return ()
    return aliases


def load_rules() -> dict:
    try:
        return json.loads(_rules_path().read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def save_rules(rules: dict) -> None:
    ts.STATE_DIR.mkdir(parents=True, exist_ok=True)
    _rules_path().write_text(json.dumps(rules, indent=2), encoding="utf-8")


def write_digest(rules: dict, path: Path) -> None:
    by_cat: dict[str, list[dict]] = defaultdict(list)
    for r in rules.values():
        by_cat[r["category"]].append(r)
    lines = ["# Adapt digest (generated by adapt; MemRight rows are the source of truth)", ""]
    for cat in sorted(by_cat):
        lines.append(f"# {cat}")
        for r in sorted(by_cat[cat], key=lambda x: -x["confidence"]):
            lines.append(f"- {r['rule']} Confidence: {r['confidence']:.2f}")
        lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def write_metrics(metrics: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")


def _audit(entry: dict) -> None:
    ts.STATE_DIR.mkdir(parents=True, exist_ok=True)
    p = _audit_file()
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "a", encoding="utf-8") as fh:
        fh.write(json.dumps({"ts": dt.datetime.now(dt.timezone.utc).isoformat(), **entry}) + "\n")


def _scope_for(obs_list: list[dict]) -> str:
    scopes = {o.get("scope") for o in obs_list if o.get("scope")}
    return scopes.pop() if len(scopes) == 1 else "D--Claude"


def _synth_committable(outcome: str) -> bool:
    return outcome in outcomes.COMMITTABLE


def _cached_synth(journal, batch_id: str) -> tuple[str, list[dict]]:
    record = journal.cached_payload(batch_id, "synthesized")
    if record is None:
        return outcomes.Outcome.PROVIDER_FAILED, []
    return record.get("synth_outcome", outcomes.Outcome.PROVIDER_FAILED), list(
        record.get("actions", [])
    )


def _replayable_synth(journal, batch_id: str) -> tuple[bool, str, list[dict]]:
    outcome, actions = _cached_synth(journal, batch_id)
    return _synth_committable(outcome), outcome, actions


def _cached_extract_progress(journal, batch_id: str) -> tuple[int, list[dict]]:
    latest_by_index: dict[int, dict] = {}
    for entry in journal.batches():
        if entry.get("batch_id") != batch_id or entry.get("stage") != "extracted":
            continue
        index = int(entry.get("batch", 0))
        if index > 0:
            latest_by_index[index] = entry

    completed_batch = 0
    observations: list[dict] = []
    for index in range(1, max(latest_by_index, default=0) + 1):
        entry = latest_by_index.get(index)
        if entry is None or not (
            "observations" in entry or entry.get("valid_empty") is True
        ):
            break
        if "observations" in entry:
            observations = list(entry["observations"])
        completed_batch = index
    return completed_batch, observations


def _extraction_contract() -> dict:
    """Stable fingerprint for deciding whether cached extraction is reusable."""
    return {
        "version": 1,
        "batch_char_budget": adapt_llm.BATCH_CHAR_BUDGET,
        "max_tokens": adapt_llm.MAX_TOKENS,
        "extract_prompt_sha256": hashlib.sha256(
            adapt_llm.EXTRACT_SYSTEM.encode("utf-8")
        ).hexdigest(),
        "synth_prompt_sha256": hashlib.sha256(
            adapt_llm.SYNTH_SYSTEM.encode("utf-8")
        ).hexdigest(),
        "synth_max_tokens": adapt_llm.SYNTH_MAX_TOKENS,
        "preference_prefilter_version": ts.PREFERENCE_PREFILTER_VERSION,
    }


def _session_refs(sessions) -> list[dict]:
    return [{
        "session_id": session.session_id,
        "tool": session.tool,
        "path_stem": session.path.stem,
        "mtime": session.mtime,
    } for session in sessions]


def _resume_mismatch_reason(discovered: dict, current_session_refs: list[dict]
                            ) -> str | None:
    cached_refs = discovered.get("session_refs")
    if cached_refs != current_session_refs:
        return "cached session identity does not match current discovery"
    if discovered.get("extraction_contract") != _extraction_contract():
        return "cached extraction contract does not match current extractor"
    return None


def _extract_batches(batches: list[list[tuple[str, str, str]]], *, lane: str,
                     journal, batch_id: str, observations: list[dict],
                     completed_batch: int, quiet: bool, workers: int
                     ) -> tuple[list[dict], list[outcomes.BatchOutcome],
                                tuple[int, outcomes.BatchOutcome] | None]:
    """Extract independent batches concurrently, checkpointing in order."""
    batch_outcomes: list[outcomes.BatchOutcome] = []
    pending = [
        (index, batch) for index, batch in enumerate(batches, 1)
        if index > completed_batch
    ]
    for offset in range(0, len(pending), workers):
        window = pending[offset:offset + workers]
        if not quiet:
            for index, batch in window:
                print(f"  extract batch {index}: {len(batch)} turns")
        if len(window) == 1:
            index, batch = window[0]
            results = {index: adapt_llm.extract_observations(batch, lane=lane)}
        else:
            with ThreadPoolExecutor(max_workers=len(window)) as pool:
                futures = {
                    index: pool.submit(adapt_llm.extract_observations, batch, lane=lane)
                    for index, batch in window
                }
                results = {}
                for index, future in futures.items():
                    try:
                        results[index] = future.result()
                    except Exception as exc:
                        results[index] = outcomes.BatchOutcome.provider_failed(
                            f"{type(exc).__name__}: {exc}"
                        )
        for index, _batch in window:
            batch_outcome = results[index]
            batch_outcomes.append(batch_outcome)
            if batch_outcome.outcome == outcomes.Outcome.SUCCESS:
                observations.extend(batch_outcome.actions)
                if journal:
                    journal.record(batch_id, "extracted", batch=index,
                                   observations=observations)
            elif batch_outcome.outcome == outcomes.Outcome.VALID_EMPTY:
                if journal:
                    journal.record(batch_id, "extracted", batch=index,
                                   valid_empty=True, observations=observations)
            else:
                if journal:
                    journal.record(batch_id, "extracted", batch=index,
                                   outcome=batch_outcome.outcome,
                                   reason=batch_outcome.reason)
                return observations, batch_outcomes, (index, batch_outcome)
    return observations, batch_outcomes, None


def apply_actions(actions: list[dict], obs_by_cat: dict, rules: dict,
                  out_dir: Path, dry_run: bool,
                  manifest_records: list | None = None,
                  source_session_ids: list[str] | None = None,
                  source_file_hashes: dict[str, str] | None = None,
                  authority_manifest: dict | None = None,
                  authority_root: Path | None = None) -> tuple[int, bool]:
    """Apply actions.

    If `manifest_records` is provided, every action is appended as a
    schema-shaped candidate (Gate 1a manifest contract) keyed on a stable
    ``PreferenceRecord`` id with a ``payload_sha256`` over the immutable
    fields (Gate 1b hardening: source_file_hashes + evidence_ids are
    included). Automated adjudication resolves status; apply refuses content edits.

    ``source_session_ids`` (defaults to []) populates each candidate's
    ``source_ids`` so manifest apply can verify the rule's provenance.
    ``source_file_hashes`` (session_id → sha256) populates the
    ``source_file_hashes[]`` block so audits can pinpoint the transcript
    the rule was mined from.
    """
    changed = 0
    ok_all = True
    canonical_names = set(rules.keys())
    src_ids = list(source_session_ids or [])
    src_hashes = dict(source_file_hashes or {})
    for act in actions:
        name, kind = act["name"], act["action"]
        if kind == "keep":
            continue
        category_observations = obs_by_cat.get(act["category"], [])
        requested_ids = act.get("observation_ids") or []
        obs_list = (
            [obs for obs in category_observations
             if obs.get("observation_id") in requested_ids]
            if requested_ids else category_observations
        )
        evidence = obs_list[0]["evidence"] if obs_list else ""
        tool = obs_list[0].get("tool", "") if obs_list else ""
        scope = rules.get(name, {}).get("scope") or _scope_for(obs_list)
        retrieval_aliases = _safe_retrieval_aliases(act["rule"], obs_list)
        linked_source_ids = list(dict.fromkeys(
            obs.get("session_id") for obs in obs_list if obs.get("session_id")
        ))
        candidate_source_ids = linked_source_ids or src_ids
        # Admission policy: refuse anything outside the controlled taxonomy,
        # empty/short/dup rules, or that doesn't pass minimum-shape checks.
        try:
            if admission is None:
                raise RuntimeError("admission module unavailable")
            admitted, why = admission.admit(
                {**act, "scope": scope},
                canonical_rules=canonical_names,
                authority_manifest=authority_manifest,
                authority_root=authority_root,
            )
        except Exception as exc:
            admitted, why = False, f"admission-error:{type(exc).__name__}"
        record = {"name": name, "category": act["category"], "rule": act["rule"],
                  "confidence": act["confidence"], "observations": act.get("observations", 1),
                  "scope": scope, "needs_review": act.get("needs_review", False),
                   "record_type": authority.normalize_record_type(act.get("record_type")),
                   "retrieval_aliases": list(retrieval_aliases),
                  "authority_effect": authority.evaluate_rule(
                      act["rule"], scope=scope,
                      declared_effect=act.get("authority_effect")
                  ).authority_effect}
        # Manifest capture: schema-shaped candidate via PreferenceRecord.
        if manifest_records is not None:
            existing = rules.get(name)
            pr = preference_record.PreferenceRecord.from_synthesis(
                {**act, "retrieval_aliases": retrieval_aliases},
                scope=scope, source_ids=tuple(candidate_source_ids),
                existing=existing,
            )
            cand = preference_record.to_manifest_candidate(
                pr, evidence_excerpt=evidence,
                status="pending",
            )
            # Gate 1b: per-session transcript hashes + stable evidence IDs.
            cand["source_file_hashes"] = [
                {"session_id": sid, "sha256": src_hashes[sid]}
                for sid in candidate_source_ids if sid in src_hashes
            ]
            cand["evidence_ids"] = [
                {
                    "evidence_id": manifest.derive_evidence_id(
                        scope, obs.get("evidence", "")
                    ),
                    "source_session_id": obs.get("session_id", ""),
                    "excerpt": obs.get("evidence", ""),
                }
                for obs in obs_list if obs.get("evidence")
            ]
            if authority_manifest:
                cand["authority_manifest_sha256"] = authority_manifest[
                    "manifest_sha256"
                ]
            # Compute and stamp payload_sha256 AFTER the Gate 1b additions so
            # the loader's candidate_payload() computes the same hash.
            cand["payload_sha256"] = manifest.payload_sha256(cand)
            cand["_admission_status"] = "pending" if admitted else "rejected-by-admission"
            cand["_rejection_reason"] = None if admitted else why
            cand["_action"] = kind
            manifest_records.append(cand)
        if not admitted:
            print(f"  reject  {name}  cat={act.get('category','?')}  reason={why}")
            _audit({"event": "admission_rejected", "name": name, "why": why,
                    "category": act.get("category", "")})
            continue
        print(f"  {kind:6s} {name}  conf={act['confidence']:.2f}  [{scope}]")
        if dry_run:
            continue
        body = rule_body(record, evidence, tool)
        with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False,
                                         encoding="utf-8", dir=str(out_dir)) as tmp:
            tmp.write(body)
            tmp_path = tmp.name
        ok = _run_memright(["put", name, "--scope", scope, "--file", tmp_path])
        Path(tmp_path).unlink(missing_ok=True)
        if ok:
            rules[name] = record
            canonical_names.add(name)
            _audit({"action": kind, **record})
            changed += 1
        else:
            ok_all = False
            _audit({"event": "memright_write_failed", "action": kind, "name": name, "scope": scope})
    return changed, ok_all


def _preflight_apply_manifest() -> bool:
    """Minimal preflight for `--apply-from-manifest`: memright + scanner only.

    The LLM lane is irrelevant — no provider calls happen on apply.
    """
    if not ts.scanner_available():
        print("error: scanner (detect-secrets/gitleaks) unavailable; "
              "refusing manifest apply", file=sys.stderr)
        return False
    if not _run_memright(["--help"]):
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
        rules_path=_rules_path(),
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
    if j_sessions != m_sessions:
        print("error: source_session_ids mismatch journal discovered sessions",
              file=sys.stderr)
        print(f"  journal:  {j_sessions}", file=sys.stderr)
        print(f"  manifest: {m_sessions}", file=sys.stderr)
        return 2
    session_refs = discovered.get("session_refs") or [
        {"session_id": sid, "tool": "claude-code", "path_stem": sid, "mtime": 0}
        for sid in j_sessions
    ]
    if sorted(ref.get("session_id") for ref in session_refs) != m_sessions:
        print("error: journal session_refs mismatch manifest sessions", file=sys.stderr)
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
            authority_root=WORKSPACE_ROOT if frozen_authority else None,
        )
        if not result.admitted:
            authority_rejected.append((rec["id"], result.reason))
    if authority_rejected:
        print("error: manifest contains authority-quarantined records:",
              file=sys.stderr)
        for record_id, reason in authority_rejected:
            print(f"  - {record_id}: {reason}", file=sys.stderr)
        return 2

    if not _preflight_apply_manifest():
        return 2

    print(f"adapt: applying manifest {manifest_path}")
    print(f"  batch_id={batch_id}, sessions={len(j_sessions)}, "
          f"accepted={len(accepted)}, rejected={len(rejected)}")

    try:
        safe_point = _create_apply_safepoint(m)
    except (OSError, RuntimeError, ValueError, sqlite3.Error) as exc:
        print(f"error: refusing manifest apply; safe-point failed: {exc}",
              file=sys.stderr)
        _audit({"event": "manifest_safepoint_failed", "batch_id": batch_id,
                "error": str(exc)})
        jrn.record(batch_id, "applied", applied=0, ok=False,
                   failed=["safepoint"])
        return 2
    _audit({"event": "manifest_safepoint_created", "batch_id": batch_id,
            "path": str(safe_point)})
    print(f"  safepoint={safe_point}")

    # Atomic write loop: write all accepted, then advance state.
    out_dir = ts.STATE_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    tmp_paths: list[Path] = []
    written: list[tuple[str, str]] = []
    failed: list[tuple[str, str]] = []

    # Cheap stub class: ts.mark_learned reads session_id, tool, path.stem, mtime.
    class _SessStub:
        def __init__(self, ref: dict) -> None:
            self.session_id = ref["session_id"]
            self.tool = ref["tool"]
            self.mtime = float(ref["mtime"])
            self.path = Path(ref["path_stem"])

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
                {**rec, "retrieval_aliases": retrieval_aliases}
            )
        except (KeyError, ValueError) as exc:
            failed.append((rec["id"], f"contract-invalid: {exc}"))
            continue
        body = preference_record.to_memright_content(pr)
        with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False,
                                         encoding="utf-8",
                                         dir=str(out_dir)) as tmp:
            tmp.write(body)
            tmp_path = tmp.name
        tmp_paths.append(Path(tmp_path))
        ok = _run_memright(["put", pr.id, "--scope", pr.scope, "--file", tmp_path])
        if ok:
            written.append((pr.id, pr.scope))
            _audit({"event": "manifest_applied", "id": pr.id,
                    "scope": pr.scope, "manifest": str(manifest_path)})
        else:
            failed.append((pr.id, "memright put failed"))
            _audit({"event": "manifest_apply_failed", "id": pr.id,
                    "scope": pr.scope})
            break

    # Cleanup tmp files regardless of outcome.
    for tp in tmp_paths:
        try:
            Path(tp).unlink(missing_ok=True)
        except Exception:
            pass

    if failed:
        # Roll back partial writes via the resident service.
        for w, scope in written:
            try:
                qualified = f"{scope}/{w}"
                if _run_memright(["delete", qualified]):
                    _audit({"event": "manifest_rollback_deleted", "id": qualified})
                else:
                    print(f"warn: failed to rollback {qualified}", file=sys.stderr)
            except OSError as exc:
                print(f"warn: failed to rollback {scope}/{w}: {exc}", file=sys.stderr)
        print(f"error: {len(failed)} put(s) failed; rolled back "
              f"{len(written)} write(s); refusing state advance",
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
        rp = _rules_path()
        if rp.exists():
            rules_obj = json.loads(rp.read_text(encoding="utf-8"))
        for rec in accepted:
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
            }
        rp.parent.mkdir(parents=True, exist_ok=True)
        rp.write_text(json.dumps(rules_obj, indent=2), encoding="utf-8")
        try:
            write_digest(rules_obj, _digest_path())
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


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--backfill", action="store_true", help="process all unlearned sessions")
    mode.add_argument("--incremental", action="store_true", help="only new/modified sessions")
    mode.add_argument("--smoke", action="store_true", help="3 most recent sessions, forced dry-run")
    mode.add_argument("--compile-core", type=Path, metavar="OUT",
                      help="compile accepted root preferences into a bounded core artifact")
    mode.add_argument("--apply-from-manifest", type=Path, default=None,
                      help="skip LLM mining and atomically apply a reviewed manifest")
    ap.add_argument("--apply", action="store_true", help="write to MemRight (default: dry-run)")
    ap.add_argument("--dry-run", action="store_true", help="explicit no-write preview")
    ap.add_argument("--limit", type=int, default=None, help="max sessions this run")
    ap.add_argument("--before-mtime", type=float, default=None,
                    help="ignore live sessions modified after this Unix timestamp")
    ap.add_argument("--lane", default="local", choices=("local", "minimax"), help="LLM lane")
    ap.add_argument("--extract-workers", type=int, choices=range(1, 6), default=1,
                    help="parallel extraction calls (1-5; synthesis stays ordered)")
    ap.add_argument("--allow-external-lane", action="store_true",
                    help="allow sending redacted transcript text to an external LLM lane")
    ap.add_argument("--first-run-ok", action="store_true",
                    help="allow the first state-initializing apply/backfill")
    ap.add_argument("--resume", action="store_true",
                    help="resume the most recent incomplete journal batch "
                         "(replay cached observations/actions instead of re-running the LLM)")
    ap.add_argument("--manifest", type=Path, default=None,
                    help="write an approval manifest (JSON) to this path; "
                         "the dry-run halts after writing; --apply consumes "
                         "a reviewed manifest via --apply-from-manifest")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    # ----- Gate 1a early branch: apply-from-manifest is its own path -----
    # No session discovery, no LLM calls, no smoke/dry-run semantics. Bound to
    # the journal so a stale manifest cannot route to the wrong batch.
    if args.apply_from_manifest:
        return apply_from_manifest(args.apply_from_manifest)

    if args.compile_core:
        if not ts.scanner_available():
            print("error: core compilation requires detect-secrets or gitleaks", file=sys.stderr)
            return 2
        if args.lane != "local" and not args.allow_external_lane:
            print("error: external core compiler lane requires --allow-external-lane",
                  file=sys.stderr)
            return 2
        if not adapt_llm.lane_available(args.lane):
            print(f"error: LLM lane unavailable: {args.lane}", file=sys.stderr)
            return 2
        try:
            result = core_compiler.compile_and_write(
                load_rules(), args.compile_core, lane=args.lane
            )
        except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as exc:
            print(f"error: core compilation failed: {exc}", file=sys.stderr)
            return 2
        print(f"adapt: compiled {len(result['rules'])} core rules "
              f"({result['estimated_tokens']} estimated tokens) -> {args.compile_core}")
        return 0

    dry_run = args.dry_run or args.smoke or not args.apply or bool(args.manifest)
    limit = 3 if args.smoke else args.limit

    state = ts.load_state()
    if args.apply and not preflight_apply(args.lane, args.allow_external_lane):
        return 2
    if args.apply and not initialized(state) and not (args.smoke or args.first_run_ok):
        print("error: first adapt apply requires --first-run-ok after smoke/dry-run review",
              file=sys.stderr)
        return 2
    if args.smoke:
        sessions = ts.new_sessions(state, limit=3, newest=True)
    else:
        sessions = ts.new_sessions(state, limit=limit, before_mtime=args.before_mtime)
    if not sessions:
        if not args.quiet:
            print("adapt: no new sessions")
        ts.save_state(state)   # persists excluded/empty markings
        return 0
    print(f"adapt: {len(sessions)} sessions "
          f"({sum(len(s.turns) for s in sessions)} turns), dry_run={dry_run}")
    drops = sum(s.stats.dropped_turns for s in sessions)
    trunc = sum(s.stats.truncated_turns for s in sessions)
    scans = sum(s.stats.scanner_drops for s in sessions)
    print(f"adapt: parser stats dropped={drops} truncated={trunc} scanner_drops={scans}")

    all_turn_count = sum(len(s.turns) for s in sessions)
    turns = []
    for session in sessions:
        for turn in session.turns:
            candidate = ts.preference_candidate_text(turn.text)
            if candidate is not None:
                turns.append((session.tool, turn.scope, candidate, session.session_id))
    print(f"adapt: preference prefilter kept={len(turns)}/{all_turn_count} turns "
          f"({sum(len(turn[2]) for turn in turns)} chars)")
    session_refs = _session_refs(sessions)
    observations: list[dict] = []
    extract_outcomes: list[outcomes.BatchOutcome] = []
    journal = (run_journal.RunJournal() if run_journal else None)
    batch_id = run_journal.new_batch_id() if run_journal else "manual"

    # Resume path: if a previous run's batch left a partial record, replay
    # from the last completed stage using the cached payload instead of
    # re-running the LLM. This is the wire-up the user flagged as missing.
    replay = None
    completed_extract_batch = 0
    if journal:
        pb = journal.pending_batch()
        if pb is not None and pb.get("batch_id") != batch_id:
            print(f"adapt: resuming prior batch {pb['batch_id']} from stage "
                  f"{pb.get('stage')} — pass --resume explicitly to retry it.",
                  file=sys.stderr)
        if args.resume and pb is not None:
            discovered = journal.cached_payload(pb["batch_id"], "discovered") or {}
            mismatch = _resume_mismatch_reason(discovered, session_refs)
            if mismatch:
                print(f"error: refusing unsafe resume of {pb['batch_id']}: {mismatch}",
                      file=sys.stderr)
                return 2
            replay = pb
            batch_id = replay["batch_id"]
            completed_extract_batch, observations = _cached_extract_progress(
                journal, batch_id
            )
            if completed_extract_batch:
                print(f"adapt: replayed {len(observations)} cached observations "
                      f"through extract batch {completed_extract_batch} from {batch_id}")

    if journal and replay is None:
        journal.record(batch_id, "discovered",
                       sessions=[s.session_id for s in sessions],
                       session_refs=session_refs,
                       extraction_contract=_extraction_contract(),
                       turn_count=len(turns))

    # ----- Stage 1: extract (ordered checkpoints over parallel windows) -----
    observations, extract_outcomes, extract_failure = _extract_batches(
        adapt_llm.build_batches(turns), lane=args.lane, journal=journal,
        batch_id=batch_id, observations=observations,
        completed_batch=completed_extract_batch, quiet=args.quiet,
        workers=args.extract_workers,
    )
    if extract_failure:
        index, batch_outcome = extract_failure
        print(f"adapt: extract batch {index} outcome={batch_outcome.outcome} "
              f"reason={batch_outcome.reason}", file=sys.stderr)
        print(f"adapt: NOT advancing state — outcome {batch_outcome.outcome}",
              file=sys.stderr)
        return 2

    print(f"adapt: {len(observations)} candidate observations")
    for index, observation in enumerate(observations, 1):
        observation["observation_id"] = f"obs-{index:06d}"

    # ----- Stage 2: synthesize ONCE with the full existing-rules list -----
    rules = load_rules()
    existing_rules_for_synth = [
        {"name": r["name"], "category": r["category"], "rule": r["rule"],
         "confidence": r.get("confidence", 0.6), "observations": r.get("observations", 1)}
        for r in rules.values()
    ]
    # Replay cached synth actions when resuming.
    actions: list[dict] = []
    replayed_synth = False
    synth_reason = ""
    if replay is not None:
        replayed_synth, synth_outcome, actions = _replayable_synth(journal, batch_id)
        if replayed_synth:
            print(f"adapt: replayed {len(actions)} cached actions from {batch_id}")
    if not replayed_synth:
        synth = adapt_llm.synthesize(existing_rules_for_synth, observations,
                                     lane=args.lane)
        synth_outcome = synth.outcome
        actions = synth.actions
        synth_reason = synth.reason
        if journal:
            # Persist the actual actions — produces are small, fully serializable.
            journal.record(batch_id, "synthesized",
                           synth_outcome=synth_outcome,
                           synth_reason=synth_reason, actions=actions)
    if not _synth_committable(synth_outcome):
        print(f"adapt: synth {synth_outcome}; reason={synth_reason}; "
              "not advancing state", file=sys.stderr)
        return 2

    # ----- Stage 3: apply with admission gating -----
    authority_snapshot = authority.build_manifest(WORKSPACE_ROOT)
    obs_by_cat = defaultdict(list)
    for o in observations:
        obs_by_cat[o["category"]].append(o)

    # Manifest mode: emit immutable candidates for automated adjudication.
    if args.manifest:
        manifest_records: list[dict] = []
        source_session_ids = [s.session_id for s in sessions]
        # Gate 1b: per-session transcript hashes for reviewer cross-reference.
        source_file_hashes = {s.session_id: s.file_sha256() for s in sessions}
        apply_actions(actions, obs_by_cat, rules, ts.STATE_DIR, dry_run,
                      manifest_records=manifest_records,
                      source_session_ids=source_session_ids,
                      source_file_hashes=source_file_hashes,
                      authority_manifest=authority_snapshot,
                      authority_root=WORKSPACE_ROOT)
        # Strip admission-metadata keys; only schema fields survive in the file.
        schema_records = []
        for r in manifest_records:
            schema_records.append({k: v for k, v in r.items()
                                   if not k.startswith("_")})
        # Mark admission-rejected records as 'rejected' so reviewers see the
        # gate decision (they cannot be flipped back to 'accepted' since the
        # payload still passes through `admission.admit()` on apply).
        for r, raw in zip(schema_records, manifest_records):
            if raw.get("_admission_status") == "rejected-by-admission":
                r["status"] = "rejected"
                if raw.get("_rejection_reason"):
                    r["human_note"] = (
                        f"admission-rejected: {raw['_rejection_reason']}"
                    )
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        body = {
            "schema_version": preference_record.SCHEMA_VERSION,
            "batch_id": batch_id,
            "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "generator": "adapt.py --manifest",
            "authority_manifest": authority_snapshot,
            "source_session_ids": source_session_ids,
            "records": schema_records,
        }
        args.manifest.write_text(json.dumps(body, indent=2, ensure_ascii=False),
                                encoding="utf-8")
        accepted = sum(1 for r in schema_records if r["status"] == "accepted")
        rejected = sum(1 for r in schema_records if r["status"] == "rejected")
        pending = sum(1 for r in schema_records if r["status"] == "pending")
        print(f"adapt: wrote {args.manifest} "
              f"({accepted} admissible / {rejected} admission-rejected / "
              f"{pending} pending review)")
        # new_sessions marks only parser-empty and deterministically excluded
        # files in this state object. Persist those skips so later chunks do
        # not repeatedly parse generated worker transcripts.
        ts.save_state(state)
        print("adapt: resolve pending records with adjudicate_manifest.py; "
              "apply only the resulting content-hashed manifest.")
        return 0

    delta, ok_all = apply_actions(actions, obs_by_cat, rules, ts.STATE_DIR, dry_run,
                                  source_session_ids=[s.session_id for s in sessions],
                                  authority_manifest=authority_snapshot,
                                  authority_root=WORKSPACE_ROOT)
    changed = delta
    if journal:
        journal.record(batch_id, "applied", applied=changed, ok=ok_all)

    if not dry_run:
        if not ok_all:
            print("error: one or more MemRight writes failed; state not advanced",
                  file=sys.stderr)
            return 1
        save_rules(rules)
        write_digest(rules, _digest_path())
        ts.mark_learned(state, sessions)
        state["initialized_at"] = state.get("initialized_at") or dt.datetime.now(dt.timezone.utc).isoformat()
        ts.save_state(state)
        if journal:
            journal.record(batch_id, "committed", applied=changed,
                           sessions=[s.session_id for s in sessions])
        print(f"adapt: applied {changed} rule changes; digest at {_digest_path()}")

    if dry_run:
        print("adapt: dry run — nothing written; re-run with --apply")
    return 0


if __name__ == "__main__":
    sys.exit(main())
