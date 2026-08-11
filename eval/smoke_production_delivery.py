"""Five-record copied-DB smoke for Adapt's production alias/core delivery path."""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from pathlib import Path


ROOT = Path(
    os.environ.get("WORKSPACE_ROOT") or next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())
).expanduser().resolve()
HERE = Path(__file__).resolve().parent
ADAPT_DIR = HERE.parent
TOOLS_LIB = ROOT / "tools/lib"
for path in (ADAPT_DIR, TOOLS_LIB):
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))

from adapt import adapt_sessions
from adapt import preference_record
from memory import adapt_core


DEFAULT_TREATMENT = ROOT / ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json"
DEFAULT_CORE = ROOT / "docs/evidence/adapt-taste-parity-2026-07-14/compiled-core.json"
DEFAULT_LIVE_DB = ROOT / "tools/.cache/memory/crypt-engine.db"
DEFAULT_CRYPT = ROOT / "tools/bin/crypt.exe"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/production-smoke"


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "production_smoke_runner")


def enrich_delivery_records(treatment: dict) -> list[dict]:
    """Preserve the full primary treatment while adding bounded support aliases."""
    records = []
    for source in treatment.get("records", []):
        support = source.get("support") or []
        if isinstance(support, dict):
            support = [support]
        record = dict(source)
        record["retrieval_aliases"] = list(
            preference_record.normalize_retrieval_aliases(
                [row.get("evidence", "") for row in support],
                rule=source.get("rule", ""),
            )
        )
        records.append(record)
    return records


def build_smoke_items(treatment: dict, limit: int = 5) -> tuple[list[dict], list[dict]]:
    excluded = {row["id"] for row in treatment.get("exclusions", [])}
    records = []
    cases = []
    for source in enrich_delivery_records(treatment):
        if source.get("id") in excluded:
            continue
        aliases = tuple(source["retrieval_aliases"])
        if not aliases:
            continue
        record = dict(source)
        record["retrieval_aliases"] = list(aliases)
        records.append(record)
        cases.append({
            "case_id": f"production-alias-{len(cases) + 1}",
            "prompt": aliases[0],
            "scope": source.get("scope", "D--Claude"),
            "expected_id": source["id"],
        })
        if len(records) == limit:
            break
    if len(records) != limit:
        raise ValueError(f"need {limit} supported non-quarantined records, found {len(records)}")
    return records, cases


def build_production_alias_db(crypt: Path, live_db: Path, db: Path,
                              treatment: dict) -> dict:
    """Create an isolated full-treatment DB through the production serializer."""
    records = enrich_delivery_records(treatment)
    alias_payload = json.dumps(
        [alias for row in records for alias in row["retrieval_aliases"]],
        ensure_ascii=False,
    )
    if not adapt_sessions.scan_batch_for_secrets_str(alias_payload):
        raise RuntimeError("secret scanner blocked production alias payload")
    runner.value_ab.snapshot_live_db(live_db, db)
    port = runner.value_ab._free_port()
    service = runner.value_ab._start_service(crypt, db, port)
    try:
        runner.value_ab._wait_ready(port)
        for record in records:
            runner.value_ab.put_pref_record(
                crypt, db, port, record["id"], record.get("scope", "D--Claude"),
                _body(record, True),
            )
    finally:
        runner.value_ab._stop_service(service)
    ok, count, msg = runner.value_ab.integrity_check(db)
    if not ok:
        raise RuntimeError(f"production alias DB integrity failed: {msg}")
    return {
        "record_count": len(records),
        "aliased_record_count": sum(bool(row["retrieval_aliases"]) for row in records),
        "rows": count,
        "integrity": msg,
    }


def _body(source: dict, with_aliases: bool) -> str:
    action = {
        "action": "add", "category": source["category"], "rule": source["rule"],
        "confidence": source.get("confidence", 0.7),
        "evidence_count": max(1, len(source.get("support") or [])),
        "record_type": source.get("record_type", "standing_preference"),
        "authority_effect": source.get("authority_effect", "neutral"),
        "retrieval_aliases": source.get("retrieval_aliases", []) if with_aliases else [],
    }
    record = preference_record.PreferenceRecord.from_synthesis(
        action, scope=source.get("scope", "D--Claude"), source_ids=(source["id"],),
        existing={"id": source["id"], "created_at": "2026-07-14T00:00:00+00:00"},
    )
    return preference_record.to_crypt_content(record)


def _run_arm(crypt: Path, live_db: Path, db: Path, cases: list[dict],
             records: list[dict], input_path: Path, *, aliases: bool):
    runner.value_ab.snapshot_live_db(live_db, db)
    port = runner.value_ab._free_port()
    service = runner.value_ab._start_service(crypt, db, port)
    try:
        runner.value_ab._wait_ready(port)
        for record in records:
            runner.value_ab.put_pref_record(
                crypt, db, port, record["id"], record.get("scope", "D--Claude"),
                _body(record, aliases),
            )
        ranked = runner.replay_all(crypt, db, port, cases, input_path)
    finally:
        runner.value_ab._stop_service(service)
    ok, count, msg = runner.value_ab.integrity_check(db)
    if not ok:
        raise RuntimeError(f"copied DB integrity failed: {msg}")
    return ranked, {"integrity": msg, "rows": count}


def _score(cases: list[dict], ranked: dict) -> dict:
    hits = 0
    ranks = []
    for case in cases:
        ids = [str(mid).rsplit("/", 1)[-1] for mid in ranked[case["case_id"]]["ranked_ids"]]
        if case["expected_id"] in ids[:5]:
            hits += 1
            ranks.append(ids.index(case["expected_id"]) + 1)
    return {"hit_at_5": hits, "total": len(cases),
            "hit_at_5_pct": round(100 * hits / len(cases), 1), "ranks": ranks}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--treatment", type=Path, default=DEFAULT_TREATMENT)
    parser.add_argument("--core", type=Path, default=DEFAULT_CORE)
    parser.add_argument("--live-db", type=Path, default=DEFAULT_LIVE_DB)
    parser.add_argument("--crypt-bin", type=Path, default=DEFAULT_CRYPT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--limit", type=int, default=5)
    args = parser.parse_args(argv)
    if not 2 <= args.limit <= 5:
        raise ValueError("smoke limit must be 2-5")
    treatment = json.loads(args.treatment.read_text(encoding="utf-8"))
    records, cases = build_smoke_items(treatment, args.limit)
    alias_payload = json.dumps(
        [alias for row in records for alias in row["retrieval_aliases"]], ensure_ascii=False
    )
    if not adapt_sessions.scan_batch_for_secrets_str(alias_payload):
        raise RuntimeError("secret scanner blocked smoke aliases")
    core = adapt_core.load_core(args.core)
    if core is None:
        raise RuntimeError("compiled core failed production validation")
    pre_ok, pre_count, pre_msg = runner.value_ab.integrity_check(args.live_db)
    if not pre_ok:
        raise RuntimeError(f"live DB integrity failed before smoke: {pre_msg}")
    args.out.mkdir(parents=True, exist_ok=True)
    baseline, baseline_integrity = _run_arm(
        args.crypt_bin, args.live_db, args.out / "baseline.db", cases, records,
        args.out / "baseline-queries.jsonl", aliases=False,
    )
    alias, alias_integrity = _run_arm(
        args.crypt_bin, args.live_db, args.out / "alias.db", cases, records,
        args.out / "alias-queries.jsonl", aliases=True,
    )
    post_ok, post_count, post_msg = runner.value_ab.integrity_check(args.live_db)
    if not post_ok or post_count != pre_count:
        raise RuntimeError(f"live DB changed during smoke: {post_msg}")
    result = {
        "record_count": len(records), "query_count": len(cases),
        "core": {"rules": len(core.source_ids), "estimated_tokens": core.estimated_tokens},
        "baseline": _score(cases, baseline), "alias": _score(cases, alias),
        "copied_db": {"baseline": baseline_integrity, "alias": alias_integrity},
        "live_db": {"rows_before": pre_count, "rows_after": post_count, "unchanged": True},
    }
    runner.write_json(args.out / "results.json", result)
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
