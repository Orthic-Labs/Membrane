"""Orthic Morph / Morph CLI — Taste mining, apply, and doctor."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import morph_llm  # noqa: E402
import morph_sessions as ts  # noqa: E402
import outcomes  # noqa: E402
try:
    import run_journal  # noqa: E402
except ImportError:
    run_journal = None
import preference_record  # noqa: E402
import authority  # noqa: E402
import core_compiler  # noqa: E402
import cross_machine  # noqa: E402
import taste  # noqa: E402
import taste_apply  # noqa: E402
import taste_mine  # noqa: E402


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
    mode.add_argument("--add-rule", metavar="RULE", default=None,
                      help="operator-authored single-rule add (skips LLM mining, still admission-gated); "
                           "requires --category; default record-type is operational_playbook (recall-gated)")
    ap.add_argument("--category", default=None,
                    help="category for --add-rule (workflow, verification, safety, architecture, "
                         "tooling, code-style, documentation, model-routing)")
    ap.add_argument("--record-type", default="operational_playbook",
                    choices=("operational_playbook", "standing_preference", "locked_decision",
                             "episodic_fact", "unclassified"),
                    help="record type for --add-rule; standing_preference reaches the always-on core")
    ap.add_argument("--scope", default=None,
                    help="workspace scope for --add-rule (default: this machine's workspace scope)")
    ap.add_argument("--machine-only", action="store_true",
                    help="for --add-rule: narrow this rule to the recording machine alone "
                         "(default: applies workspace-wide, unchanged from today)")
    ap.add_argument("--apply", action="store_true", help="write to Crypt (default: dry-run)")
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
    ap.add_argument("--restart-stale", action="store_true",
                    help="with --resume, abandon only a session-identity-stale journal batch "
                         "without advancing state, then start a fresh batch")
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
        return taste_apply.apply_from_manifest(args.apply_from_manifest)

    if args.add_rule is not None:
        if not args.category:
            print("error: --add-rule requires --category", file=sys.stderr)
            return 2
        # Default is dry-run like the rest of the CLI; --apply writes.
        return taste.add_rule(args.add_rule, args.category, record_type=args.record_type,
                        scope=args.scope, dry_run=not args.apply,
                        machine_only=args.machine_only)

    if args.compile_core:
        if not ts.scanner_available():
            print("error: core compilation requires detect-secrets or gitleaks", file=sys.stderr)
            return 2
        if args.lane != "local" and not args.allow_external_lane:
            print("error: external core compiler lane requires --allow-external-lane",
                  file=sys.stderr)
            return 2
        if not morph_llm.lane_available(args.lane):
            print(f"error: LLM lane unavailable: {args.lane}", file=sys.stderr)
            return 2
        try:
            result = core_compiler.compile_and_write(
                taste.load_rules(), args.compile_core, lane=args.lane
            )
        except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as exc:
            print(f"error: core compilation failed: {exc}", file=sys.stderr)
            return 2
        print(f"morph: compiled {len(result['rules'])} core rules "
              f"({result['estimated_tokens']} estimated tokens) -> {args.compile_core}")
        return 0

    dry_run = args.dry_run or args.smoke or not args.apply or bool(args.manifest)
    limit = 3 if args.smoke else args.limit

    state = ts.load_state()
    if args.apply and not taste.preflight_apply(args.lane, args.allow_external_lane):
        return 2
    if args.apply and not taste.initialized(state) and not (args.smoke or args.first_run_ok):
        print("error: first morph apply requires --first-run-ok after smoke/dry-run review",
              file=sys.stderr)
        return 2
    if args.smoke:
        sessions = ts.new_sessions(state, limit=3, newest=True)
    else:
        sessions = ts.new_sessions(state, limit=limit, before_mtime=args.before_mtime)
    if not sessions:
        if not args.quiet:
            print("morph: no new sessions")
        ts.save_state(state)   # persists excluded/empty markings
        return 0
    print(f"morph: {len(sessions)} sessions "
          f"({sum(len(s.turns) for s in sessions)} turns), dry_run={dry_run}")
    drops = sum(s.stats.dropped_turns for s in sessions)
    trunc = sum(s.stats.truncated_turns for s in sessions)
    scans = sum(s.stats.scanner_drops for s in sessions)
    print(f"morph: parser stats dropped={drops} truncated={trunc} scanner_drops={scans}")

    all_turn_count = sum(len(s.turns) for s in sessions)
    turns = []
    session_source_keys = taste_mine._session_source_keys(sessions)
    for session, source_key in zip(sessions, session_source_keys):
        for turn in session.turns:
            candidate = ts.preference_candidate_text(turn.text)
            if candidate is not None:
                turns.append((session.tool, turn.scope, candidate, source_key))
    print(f"morph: preference prefilter kept={len(turns)}/{all_turn_count} turns "
          f"({sum(len(turn[2]) for turn in turns)} chars)")
    session_refs = taste_mine._session_refs(sessions)
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
            print(f"morph: resuming prior batch {pb['batch_id']} from stage "
                  f"{pb.get('stage')} — pass --resume explicitly to retry it.",
                  file=sys.stderr)
        if args.resume and pb is not None:
            discovered = journal.cached_payload(pb["batch_id"], "discovered") or {}
            mismatch = taste_mine._resume_mismatch_reason(discovered, session_refs)
            if mismatch:
                if (args.restart_stale and mismatch ==
                        "cached session identity does not match current discovery"):
                    journal.record(
                        pb["batch_id"], "abandoned", reason="session_identity_changed"
                    )
                    print(f"morph: abandoned stale journal batch {pb['batch_id']} "
                          "without advancing session state")
                else:
                    print(f"error: refusing unsafe resume of {pb['batch_id']}: {mismatch}",
                          file=sys.stderr)
                    return 2
            else:
                replay = pb
                batch_id = replay["batch_id"]
                completed_extract_batch, observations = taste_mine._cached_extract_progress(
                    journal, batch_id
                )
                if completed_extract_batch:
                    print(f"morph: replayed {len(observations)} cached observations "
                          f"through extract batch {completed_extract_batch} from {batch_id}")

    if journal and replay is None:
        journal.record(batch_id, "discovered",
                       sessions=session_source_keys,
                       session_refs=session_refs,
                       extraction_contract=taste_mine._extraction_contract(),
                       turn_count=len(turns))

    # ----- Stage 1: extract (ordered checkpoints over parallel windows) -----
    observations, extract_outcomes, extract_failure = taste_mine._extract_batches(
        morph_llm.build_batches(turns), lane=args.lane, journal=journal,
        batch_id=batch_id, observations=observations,
        completed_batch=completed_extract_batch, quiet=args.quiet,
        workers=args.extract_workers,
    )
    if extract_failure:
        index, batch_outcome = extract_failure
        print(f"morph: extract batch {index} outcome={batch_outcome.outcome} "
              f"reason={batch_outcome.reason}", file=sys.stderr)
        print(f"morph: NOT advancing state — outcome {batch_outcome.outcome}",
              file=sys.stderr)
        return 2

    print(f"morph: {len(observations)} candidate observations")
    for index, observation in enumerate(observations, 1):
        observation["observation_id"] = f"obs-{index:06d}"

    # ----- Stage 2: synthesize ONCE with the full canonical existing-rules list -----
    try:
        multiwriter_context = taste._multiwriter_context()
    except cross_machine.CrossMachineMorphError as exc:
        print(f"error: canonical Morph context unavailable: {exc}", file=sys.stderr)
        return 2
    if multiwriter_context is not None:
        installation_id, rules = multiwriter_context
    else:
        installation_id = None
        rules = taste.load_rules()
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
        replayed_synth, synth_outcome, actions = taste_mine._replayable_synth(journal, batch_id)
        if replayed_synth:
            print(f"morph: replayed {len(actions)} cached actions from {batch_id}")
    if not replayed_synth:
        synth = morph_llm.synthesize(existing_rules_for_synth, observations,
                                     lane=args.lane)
        synth_outcome = synth.outcome
        actions = synth.actions
        synth_reason = synth.reason
        if journal:
            # Persist the actual actions — produces are small, fully serializable.
            journal.record(batch_id, "synthesized",
                           synth_outcome=synth_outcome,
                           synth_reason=synth_reason, actions=actions,
                           **synth.provider_receipt())
    if not taste_mine._synth_committable(synth_outcome):
        print(f"morph: synth {synth_outcome}; reason={synth_reason}; "
              "not advancing state", file=sys.stderr)
        return 2

    if installation_id is not None and args.apply and not args.manifest:
        print(
            "error: schema-v2 installations apply mined preferences only through "
            "--manifest and --apply-from-manifest",
            file=sys.stderr,
        )
        return 2

    # ----- Stage 3: apply with admission gating -----
    authority_snapshot = authority.build_manifest(taste.WORKSPACE_ROOT)
    obs_by_cat = defaultdict(list)
    for o in observations:
        obs_by_cat[o["category"]].append(o)

    # Manifest mode: emit immutable candidates for automated adjudication.
    if args.manifest:
        manifest_records: list[dict] = []
        if installation_id is not None:
            source_map = {
                ref["source_key"]: cross_machine.qualify_source_session(
                    installation_id, ref["tool"], ref["source_key"]
                )
                for ref in session_refs
            }
            source_session_ids = [source_map[ref["source_key"]] for ref in session_refs]
            for observation in observations:
                raw_source = observation.get("session_id")
                if raw_source in source_map:
                    observation["session_id"] = source_map[raw_source]
        else:
            source_map = {ref["source_key"]: ref["source_key"] for ref in session_refs}
            source_session_ids = [ref["source_key"] for ref in session_refs]
        # Gate 1b: per-session transcript hashes for reviewer cross-reference.
        source_file_hashes = {
            source_map[ref["source_key"]]: session.file_sha256()
            for session, ref in zip(sessions, session_refs)
        }
        taste.apply_actions(actions, obs_by_cat, rules, ts.STATE_DIR, dry_run,
                      manifest_records=manifest_records,
                      source_session_ids=source_session_ids,
                      source_file_hashes=source_file_hashes,
                      authority_manifest=authority_snapshot,
                      authority_root=taste.WORKSPACE_ROOT)
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
            "generator": "morph.py --manifest",
            "authority_manifest": authority_snapshot,
            "source_session_ids": source_session_ids,
            "records": schema_records,
        }
        if installation_id is not None:
            body["installation_id"] = installation_id
            body["canonical_pool_sha256"] = cross_machine.canonical_pool_sha256(
                rules
            )
        args.manifest.write_text(json.dumps(body, indent=2, ensure_ascii=False),
                                encoding="utf-8")
        accepted = sum(1 for r in schema_records if r["status"] == "accepted")
        rejected = sum(1 for r in schema_records if r["status"] == "rejected")
        pending = sum(1 for r in schema_records if r["status"] == "pending")
        print(f"morph: wrote {args.manifest} "
              f"({accepted} admissible / {rejected} admission-rejected / "
              f"{pending} pending review)")
        # new_sessions marks only parser-empty and deterministically excluded
        # files in this state object. Persist those skips so later chunks do
        # not repeatedly parse generated worker transcripts.
        ts.save_state(state)
        print("morph: resolve pending records with adjudicate_manifest.py; "
              "apply only the resulting content-hashed manifest.")
        return 0

    delta, ok_all = taste.apply_actions(actions, obs_by_cat, rules, ts.STATE_DIR, dry_run,
                                  source_session_ids=[s.session_id for s in sessions],
                                  authority_manifest=authority_snapshot,
                                  authority_root=taste.WORKSPACE_ROOT)
    changed = delta
    if journal:
        journal.record(batch_id, "applied", applied=changed, ok=ok_all)

    if not dry_run:
        if not ok_all:
            print("error: one or more Crypt writes failed; state not advanced",
                  file=sys.stderr)
            return 1
        taste.save_rules(rules)
        taste.write_digest(rules, taste._digest_path())
        ts.mark_learned(state, sessions)
        state["initialized_at"] = state.get("initialized_at") or dt.datetime.now(dt.timezone.utc).isoformat()
        ts.save_state(state)
        if journal:
            journal.record(batch_id, "committed", applied=changed,
                           sessions=[s.session_id for s in sessions])
        print(f"morph: applied {changed} rule changes; digest at {taste._digest_path()}")

    if dry_run:
        print("morph: dry run — nothing written; re-run with --apply")
    return 0



def _dispatch(argv: list[str] | None = None) -> int:
    """Support `morph doctor ...` / `morph doctor ...` plus legacy flag CLI."""
    import doctor as morph_doctor

    args = list(sys.argv[1:] if argv is None else argv)
    if args and args[0] in {"doctor", "doc"}:
        return morph_doctor.main(args[1:])
    # Legacy flag-based Taste CLI expects sys.argv; temporarily rewrite when argv given.
    if argv is not None:
        old = sys.argv
        try:
            sys.argv = [old[0], *argv]
            return main()
        finally:
            sys.argv = old
    return main()


if __name__ == "__main__":
    raise SystemExit(_dispatch())
