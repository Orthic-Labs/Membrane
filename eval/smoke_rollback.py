"""Rollback smoke for synthetic or adjudicated records on a copied MemRight DB."""
from __future__ import annotations

import argparse
import datetime as dt
import importlib.util
import json
import os
import sqlite3
import sys
from pathlib import Path


ROOT = Path(r"D:\Claude")
ADAPT = ROOT / "tools/pipelines/memory/adapt"
sys.path.insert(0, str(ADAPT))

import adapt as adapt_pipeline
import adapt_sessions
import manifest as manifest_contract
import preference_record
import rollback
import run_journal


def _load_runner():
    path = Path(__file__).resolve().parent / "run_value_ab.py"
    spec = importlib.util.spec_from_file_location("rollback_smoke_runner", path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


runner = _load_runner()


def _candidate(index: int) -> dict:
    source_id = f"rollback-smoke-source-{index}"
    stored = preference_record.PreferenceRecord.from_synthesis(
        {
            "category": "verification",
            "rule": f"Always verify rollback smoke record {index} through the isolated service.",
            "confidence": 0.9,
            "record_type": "operational_playbook",
            "authority_effect": "neutral",
            "evidence_count": 1,
            "retrieval_aliases": [f"rollback smoke evidence {index}"],
        },
        scope="D--Claude",
        source_ids=(source_id,),
    )
    candidate = preference_record.to_manifest_candidate(
        stored,
        evidence_excerpt=f"Always verify rollback smoke record {index}.",
        status="accepted",
    )
    candidate["payload_sha256"] = manifest_contract.payload_sha256(candidate)
    return candidate


def _memory_ids(db: Path) -> set[str]:
    with sqlite3.connect(db) as conn:
        return {row[0] for row in conn.execute("SELECT id FROM memories")}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live-db", type=Path, default=runner.DEFAULT_LIVE_DB)
    parser.add_argument("--memright-bin", type=Path, default=runner.DEFAULT_MEMRIGHT)
    parser.add_argument("--manifest", type=Path,
                        help="resolved production manifest to apply/retrieve/revert")
    parser.add_argument("--out", type=Path,
                        default=ROOT / ".cache/adapt-delivery-parity/rollback-smoke")
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)

    live_ok, live_before, message = runner.integrity_check(args.live_db)
    if not live_ok:
        raise RuntimeError(f"live DB failed preflight: {message}")
    copied_db = args.out / "copied.db"
    copied_before = runner.snapshot_live_db(args.live_db, copied_db)
    state_dir = args.out / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    state = state_dir / "state.json"
    rules = state_dir / "rules.json"
    core = state_dir / "core.json"
    journal_path = state_dir / "run_journal.jsonl"
    journal_path.unlink(missing_ok=True)
    state.write_text('{"learned":{}}', encoding="utf-8")
    rules.write_text("{}", encoding="utf-8")
    core.write_text('{"schema_version":"1.0.0"}', encoding="utf-8")

    if args.manifest:
        manifest_path = args.manifest.resolve()
        manifest_body = manifest_contract.apply_time_validate(manifest_path)
        records = manifest_contract.accepted_records(manifest_body)
        if not records:
            raise RuntimeError("resolved manifest has no accepted records")
        batch_id = manifest_body["batch_id"]
        source_ids = manifest_body["source_session_ids"]
    else:
        records = [_candidate(1), _candidate(2)]
        batch_id = "rollback-exact-smoke"
        source_ids = [record["source_ids"][0] for record in records]
        manifest_body = {
            "schema_version": "1.2.0",
            "batch_id": batch_id,
            "source_session_ids": source_ids,
            "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "generator": "adapt-rollback-exact-smoke",
            "records": records,
        }
        manifest_path = args.out / "manifest.json"
        manifest_path.write_text(json.dumps(manifest_body, indent=2), encoding="utf-8")
        manifest_contract.apply_time_validate(manifest_path)
    stored = [preference_record.from_manifest_candidate(record) for record in records]
    expected_ids = {f"{record.scope}/{record.id}" for record in stored}

    port = runner._free_port()
    service = runner._start_service(args.memright_bin, copied_db, port)
    env_keys = {
        "MEMRIGHT_PORT": str(port),
        "WORKSPACE_MEMORY_PORT": str(port),
        "ADAPT_SAFEPOINT_DB_OVERRIDE": str(copied_db),
        "ADAPT_SAFEPOINT_DIR_OVERRIDE": str(args.out / "safepoints"),
        "ADAPT_AUDIT_FILE_OVERRIDE": str(args.out / "audit.jsonl"),
    }
    old_env = {key: os.environ.get(key) for key in env_keys}
    try:
        runner._wait_ready(port)
        os.environ.update(env_keys)
        adapt_sessions.STATE_DIR = state_dir
        adapt_sessions.STATE_FILE = state
        run_journal.JOURNAL_FILE = journal_path
        journal = run_journal.RunJournal(journal_path)
        journal.record(batch_id, "discovered", sessions=source_ids)
        apply_rc = adapt_pipeline.apply_from_manifest(manifest_path)
        if apply_rc != 0:
            raise RuntimeError(f"production apply_from_manifest returned {apply_rc}")
        if not expected_ids.issubset(_memory_ids(copied_db)):
            raise RuntimeError("isolated apply did not persist all expected records")
        replay_rows = []
        for index, (record, stored_record) in enumerate(zip(records, stored), 1):
            aliases = record.get("retrieval_aliases") or []
            query = (aliases[0] if aliases else
                     record.get("evidence_excerpt") or record["rule"])
            replay_rows.append({
                "row_id": f"accepted-{index}", "query": query,
                "scope": stored_record.scope,
                "expected_id": f"{stored_record.scope}/{stored_record.id}",
            })
        replay_input = args.out / "retrieval.jsonl"
        replay_input.write_text("".join(
            json.dumps({key: row[key] for key in ("row_id", "query", "scope")}) + "\n"
            for row in replay_rows
        ), encoding="utf-8")
        replay_output = runner._run_via_port(
            args.memright_bin, copied_db, port,
            ["replay", "--input", str(replay_input), "-k", "5"],
        )
        ranked = {
            item["row_id"]: item["ranked_ids"]
            for item in (json.loads(line) for line in replay_output.splitlines() if line.strip())
        }
        retrieval = [{
            **row,
            "ranked_ids": ranked.get(row["row_id"], []),
            "hit_at_5": row["expected_id"] in ranked.get(row["row_id"], []),
        } for row in replay_rows]
        if not all(item["hit_at_5"] for item in retrieval):
            raise RuntimeError("accepted record failed copied-DB retrieval smoke")
        safe_point = args.out / "safepoints" / f"{batch_id}.json"
        if not safe_point.exists():
            raise RuntimeError("automatic manifest apply did not create its safe-point")
        core.write_text("post-apply", encoding="utf-8")
        rc = rollback.revert(
            safe_point, apply=True, state_path=state, rules_path=rules,
            core_path=core, memright_bin=args.memright_bin,
        )
        if rc != 0:
            raise RuntimeError(f"rollback returned {rc}")
    finally:
        for key, old_value in old_env.items():
            if old_value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old_value
        runner._stop_service(service)

    remaining = expected_ids & _memory_ids(copied_db)
    copied_ok, copied_after, copied_message = runner.integrity_check(copied_db)
    live_ok, live_after, live_message = runner.integrity_check(args.live_db)
    result = {
        "records": len(records),
        "manifest": str(manifest_path),
        "expected_ids": sorted(expected_ids),
        "retrieval": retrieval,
        "remaining_after_rollback": sorted(remaining),
        "snapshots_restored": {
            "state": state.read_text(encoding="utf-8") == '{"learned":{}}',
            "rules": rules.read_text(encoding="utf-8") == "{}",
            "core": core.read_text(encoding="utf-8") == '{"schema_version":"1.0.0"}',
        },
        "copied_db": {
            "rows_before": copied_before, "rows_after": copied_after,
            "integrity": copied_message,
        },
        "live_db": {
            "rows_before": live_before, "rows_after": live_after,
            "integrity": live_message, "unchanged": live_before == live_after,
        },
    }
    passed = (not remaining and all(item["hit_at_5"] for item in retrieval)
              and copied_ok and live_ok and copied_before == copied_after
              and live_before == live_after and all(result["snapshots_restored"].values()))
    result["passed"] = passed
    (args.out / "results.json").write_text(
        json.dumps(result, indent=2), encoding="utf-8"
    )
    print(json.dumps(result, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
