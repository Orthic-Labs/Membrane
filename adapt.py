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
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import defaultdict
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

RULES_FILE = ts.STATE_DIR / "rules.json"
DIGEST_FILE = ts.STATE_DIR / "adapt-digest.md"
WORKSPACE_ROOT = Path(__file__).resolve().parents[4]


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
        res = subprocess.run([bin_path, *args], capture_output=True, text=True, timeout=30)
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
    return (
        f"**[adapt/{rule['category']}]** — {rule['rule']} "
        f"Confidence: {rule['confidence']:.2f} "
        f"(observations: {rule['observations']}, needs_review: {str(rule.get('needs_review', False)).lower()}, "
        f"updated {today})\n"
        f"**Why:** mined from {tool or 'session'} prompts; e.g. \"{evidence}\"\n"
        f"**Record:** type={rule.get('record_type', 'unclassified')}, "
        f"authority_effect={rule.get('authority_effect', 'neutral')}\n"
        f"**How to apply:** {preference_record.application_guidance(rule.get('record_type', 'unclassified'))}\n"
    )


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
    completed_batch = 0
    observations: list[dict] = []
    for entry in journal.batches():
        if entry.get("batch_id") != batch_id or entry.get("stage") != "extracted":
            continue
        if "observations" in entry:
            observations = list(entry["observations"])
        if "observations" in entry or entry.get("valid_empty") is True:
            completed_batch = max(completed_batch, int(entry.get("batch", 0)))
    return completed_batch, observations


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
    included). Reviewers flip status to accepted/rejected; apply refuses edits.

    ``source_session_ids`` (defaults to []) populates each candidate's
    ``source_ids`` so manifest apply can verify the rule's provenance.
    ``source_file_hashes`` (session_id → sha256) populates the
    ``source_file_hashes[]`` block so reviewers can pinpoint the transcript
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
        obs_list = obs_by_cat.get(act["category"], [])
        evidence = obs_list[0]["evidence"] if obs_list else ""
        tool = obs_list[0].get("tool", "") if obs_list else ""
        scope = rules.get(name, {}).get("scope") or _scope_for(obs_list)
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
                  "authority_effect": authority.evaluate_rule(
                      act["rule"], scope=scope,
                      declared_effect=act.get("authority_effect")
                  ).authority_effect}
        # Manifest capture: schema-shaped candidate via PreferenceRecord.
        if manifest_records is not None:
            existing = rules.get(name)
            pr = preference_record.PreferenceRecord.from_synthesis(
                act, scope=scope, source_ids=tuple(src_ids),
                existing=existing,
            )
            cand = preference_record.to_manifest_candidate(
                pr, evidence_excerpt=evidence,
                status="pending",
            )
            # Gate 1b: per-session transcript hashes + stable evidence IDs.
            cand["source_file_hashes"] = [
                {"session_id": sid, "sha256": src_hashes[sid]}
                for sid in src_ids if sid in src_hashes
            ]
            cand["evidence_ids"] = [{
                "evidence_id": manifest.derive_evidence_id(scope, evidence),
                "source_session_id": (obs_list[0].get("session_id", src_ids[0])
                                       if obs_list else src_ids[0] or ""),
                "excerpt": evidence,
            }] if evidence else []
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

    # Empty accepted set: log the no-op and exit 0 — no state change.
    if not accepted:
        jrn.record(batch_id, "applied", applied=0, ok=True)
        jrn.record(batch_id, "committed", applied=0, sessions=j_sessions)
        print("adapt: no accepted records — exiting 0 with no MemRight "
              "writes and no state advance")
        return 0

    # Atomic write loop: write all accepted, then advance state.
    out_dir = ts.STATE_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    tmp_paths: list[Path] = []
    written: list[str] = []
    failed: list[tuple[str, str]] = []

    # Cheap stub class: ts.mark_learned reads session_id, tool, path.stem, mtime.
    class _SessStub:
        def __init__(self, sid: str) -> None:
            self.session_id = sid
            self.tool = "claude-code"
            self.mtime = 0

            class _P:
                stem = sid  # mark_learned uses path.stem as the per-tool key
            self.path = _P()

    for rec in accepted:
        try:
            pr = preference_record.PreferenceRecord.from_synthesis(
                {"action": "add", "name": rec["id"],
                 "category": rec["category"], "rule": rec["rule"],
                 "confidence": rec.get("confidence", 0.6),
                 "record_type": rec.get("record_type", "unclassified"),
                 "authority_effect": rec.get("authority_effect", "neutral")},
                scope=rec["scope"],
                source_ids=tuple(rec.get("source_ids", [])),
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
            written.append(pr.id)
            _audit({"event": "manifest_applied", "id": pr.id,
                    "scope": pr.scope, "manifest": str(manifest_path)})
        else:
            failed.append((pr.id, "memright put failed"))
            _audit({"event": "manifest_apply_failed", "id": pr.id,
                    "scope": pr.scope})

    # Cleanup tmp files regardless of outcome.
    for tp in tmp_paths:
        try:
            Path(tp).unlink(missing_ok=True)
        except Exception:
            pass

    if failed:
        # Roll back partial writes via the resident service.
        for w in written:
            try:
                subprocess.run(["memright", "delete", w],
                               capture_output=True, text=True, timeout=30)
                _audit({"event": "manifest_rollback_deleted", "id": w})
            except (OSError, subprocess.TimeoutExpired) as exc:
                print(f"warn: failed to rollback {w}: {exc}", file=sys.stderr)
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
    sess_stubs = [_SessStub(sid) for sid in j_sessions]
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
               names=written)
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
    ap.add_argument("--apply", action="store_true", help="write to MemRight (default: dry-run)")
    ap.add_argument("--dry-run", action="store_true", help="explicit no-write preview")
    ap.add_argument("--limit", type=int, default=None, help="max sessions this run")
    ap.add_argument("--lane", default="local", choices=("local", "minimax"), help="LLM lane")
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
    ap.add_argument("--apply-from-manifest", type=Path, default=None,
                    help="skip the LLM run entirely; apply the reviewed set "
                         "from this manifest JSON. The manifest must have "
                         "each action's status set to 'accepted'")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    # ----- Gate 1a early branch: apply-from-manifest is its own path -----
    # No session discovery, no LLM calls, no smoke/dry-run semantics. Bound to
    # the journal so a stale manifest cannot route to the wrong batch.
    if args.apply_from_manifest:
        return apply_from_manifest(args.apply_from_manifest)

    dry_run = args.dry_run or args.smoke or not args.apply or bool(args.manifest)
    limit = 3 if args.smoke else args.limit

    state = ts.load_state()
    if args.apply and not preflight_apply(args.lane, args.allow_external_lane):
        return 2
    if args.apply and not initialized(state) and not (args.smoke or args.first_run_ok):
        print("error: first adapt apply requires --first-run-ok after smoke/dry-run review",
              file=sys.stderr)
        return 2
    sessions = ts.new_sessions(state, limit=limit)
    if args.smoke:
        sessions = sessions[-3:]
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

    turns = [(s.tool, t.scope, t.text) for s in sessions for t in s.turns]
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
                       turn_count=len(turns))

    # ----- Stage 1: extract (per-batch BatchOutcome; abort if any non-committable) -----
    for i, batch in enumerate(adapt_llm.build_batches(turns), 1):
        if i <= completed_extract_batch:
            continue
        if not args.quiet:
            print(f"  extract batch {i}: {len(batch)} turns")
        bx = adapt_llm.extract_observations(batch, lane=args.lane)
        extract_outcomes.append(bx)
        if bx.outcome == outcomes.Outcome.SUCCESS:
            observations.extend(bx.actions)
            if journal:
                journal.record(batch_id, "extracted", batch=i,
                               observations=observations)
        elif bx.outcome == outcomes.Outcome.VALID_EMPTY:
            if journal:
                journal.record(batch_id, "extracted", batch=i, valid_empty=True,
                               observations=observations)
        else:
            print(f"adapt: extract batch {i} outcome={bx.outcome} reason={bx.reason}",
                  file=sys.stderr)
            if journal:
                journal.record(batch_id, "extracted", batch=i,
                               outcome=bx.outcome, reason=bx.reason)
            print(f"adapt: NOT advancing state — outcome {bx.outcome}", file=sys.stderr)
            return 2

    print(f"adapt: {len(observations)} candidate observations")

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
    if replay is not None:
        replayed_synth, synth_outcome, actions = _replayable_synth(journal, batch_id)
        if replayed_synth:
            print(f"adapt: replayed {len(actions)} cached actions from {batch_id}")
    if not replayed_synth:
        synth = adapt_llm.synthesize(existing_rules_for_synth, observations,
                                     lane=args.lane)
        synth_outcome = synth.outcome
        actions = synth.actions
        if journal:
            # Persist the actual actions — produces are small, fully serializable.
            journal.record(batch_id, "synthesized",
                           synth_outcome=synth_outcome,
                           actions=actions)
    if not _synth_committable(synth_outcome):
        print(f"adapt: synth {synth_outcome}; not advancing state", file=sys.stderr)
        return 2

    # ----- Stage 3: apply with admission gating -----
    authority_snapshot = authority.build_manifest(WORKSPACE_ROOT)
    obs_by_cat = defaultdict(list)
    for o in observations:
        obs_by_cat[o["category"]].append(o)

    # Manifest mode: emit a review file then halt. The operator flips each
    # entry's `status` to accepted / rejected and runs --apply-from-manifest.
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
        print("adapt: review each record; flip status from 'pending' to "
              f"accepted/rejected; re-run with --apply-from-manifest "
              f"{args.manifest} to write only the accepted set.")
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
