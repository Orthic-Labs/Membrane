"""Exhaustive copied-DB delivery census for full Taste and Morph treatments."""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parents[5]  # workspace root — hardcoded D:\Claude broke non-Windows checkouts
HERE = Path(__file__).resolve().parent
DEFAULT_TASTE = ROOT / "docs/evidence/commandcode-taste-bakeoff-2026-07-13/minimax-m3/root-taste.md"
DEFAULT_TREATMENT = ROOT / ".cache/morph-delivery-parity/full/frozen/morph-treatment.json"
DEFAULT_PRODUCTION_DB = ROOT / ".cache/morph-delivery-parity/production-g-deepseek/production-alias.db"
DEFAULT_LIVE_DB = ROOT / "tools/.cache/memory/crypt-engine.db"
DEFAULT_OUT = ROOT / ".cache/morph-delivery-parity/full-treatment-census"


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "census_runner")
evidence = _load(HERE / "evaluate_delivery_evidence_recall.py", "census_evidence")


def load_sources(taste_path: Path, treatment_path: Path):
    taste = runner.value_ab.parse_taste_md(taste_path)
    treatment = json.loads(treatment_path.read_text(encoding="utf-8"))
    records = treatment["records"]
    if len(taste) != 52 or len(records) != 57:
        raise ValueError(f"unexpected treatment sizes: taste={len(taste)} morph={len(records)}")
    return taste, records


def load_taste_artifact_facts(taste_path: Path) -> dict:
    metrics_path = taste_path.parent / "metrics.json"
    artifact = json.loads(metrics_path.read_text(encoding="utf-8"))["artifact"]
    return {
        "product_reported_categories": 32,
        "artifact_heading_occurrences": artifact["root_heading_occurrences"],
        "artifact_unique_headings": artifact["root_unique_headings"],
    }


def build_taste_cases(rules) -> list[dict]:
    return [
        {"case_id": f"taste-census-{index:02d}", "prompt": rule.text,
         "scope": runner.value_ab.SCOPE, "expected_id": rule.memory_id,
         "category": rule.category}
        for index, rule in enumerate(rules, 1)
    ]


def _run_taste(crypt: Path, live_db: Path, db: Path, rules, cases, out: Path):
    runner.value_ab.snapshot_live_db(live_db, db)
    port = runner.value_ab._free_port()
    service = runner.value_ab._start_service(crypt, db, port)
    try:
        runner.value_ab._wait_ready(port)
        for rule in rules:
            runner.value_ab.put_rule(crypt, db, port, rule)
        return runner.replay_all(crypt, db, port, cases, out / "taste-queries.jsonl")
    finally:
        runner.value_ab._stop_service(service)


def _run_existing(crypt: Path, source_db: Path, db: Path, cases, out: Path):
    runner.value_ab.snapshot_live_db(source_db, db)
    port = runner.value_ab._free_port()
    service = runner.value_ab._start_service(crypt, db, port)
    try:
        runner.value_ab._wait_ready(port)
        return runner.replay_all(crypt, db, port, cases, out / "morph-queries.jsonl")
    finally:
        runner.value_ab._stop_service(service)


def score_taste(cases, ranked, k: int = 5) -> dict:
    rows = []
    by_category = defaultdict(lambda: {"cases": 0, "hits": 0})
    for case in cases:
        ids = ranked[case["case_id"]]["ranked_ids"][:k]
        rank = ids.index(case["expected_id"]) + 1 if case["expected_id"] in ids else 0
        rows.append({**case, "rank": rank, "hit": bool(rank)})
        by_category[case["category"]]["cases"] += 1
        by_category[case["category"]]["hits"] += int(bool(rank))
    return {
        "rules": len(rows), "categories": len(by_category),
        "hit_at_1": sum(row["rank"] == 1 for row in rows),
        "hit_at_5": sum(row["hit"] for row in rows),
        "hit_at_5_pct": round(100 * sum(row["hit"] for row in rows) / len(rows), 1),
        "category_coverage": {
            key: {**value, "complete": value["cases"] == value["hits"]}
            for key, value in sorted(by_category.items())
        },
        "misses": [row["case_id"] for row in rows if not row["hit"]],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--taste", type=Path, default=DEFAULT_TASTE)
    parser.add_argument("--treatment", type=Path, default=DEFAULT_TREATMENT)
    parser.add_argument("--production-db", type=Path, default=DEFAULT_PRODUCTION_DB)
    parser.add_argument("--live-db", type=Path, default=DEFAULT_LIVE_DB)
    parser.add_argument("--crypt-bin", type=Path, default=runner.DEFAULT_CRYPT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--limit", type=int, default=None)
    args = parser.parse_args(argv)
    taste, morph = load_sources(args.taste, args.treatment)
    taste_facts = load_taste_artifact_facts(args.taste)
    if args.limit is not None:
        if not 2 <= args.limit <= 5:
            raise ValueError("smoke limit must be 2-5")
        taste, morph = taste[:args.limit], morph[:args.limit]
    taste_cases = build_taste_cases(taste)
    morph_cases = evidence.build_cases(morph)
    args.out.mkdir(parents=True, exist_ok=True)
    pre_ok, pre_count, pre_msg = runner.value_ab.integrity_check(args.live_db)
    if not pre_ok:
        raise RuntimeError(f"live DB integrity failed before census: {pre_msg}")
    taste_ranked = _run_taste(
        args.crypt_bin, args.live_db, args.out / "taste.db", taste, taste_cases, args.out
    )
    morph_ranked = _run_existing(
        args.crypt_bin, args.production_db, args.out / "morph.db", morph_cases, args.out
    )
    post_ok, post_count, post_msg = runner.value_ab.integrity_check(args.live_db)
    if not post_ok or post_count != pre_count:
        raise RuntimeError(f"live DB changed during census: {post_msg}")
    morph_score = evidence.score_cases(morph_cases, morph_ranked)
    result = {
        "taste": {**score_taste(taste_cases, taste_ranked), **taste_facts},
        "morph": {"records": len(morph), "queries": len(morph_cases),
                  "metrics": morph_score["metrics"],
                  "misses": [row["case_id"] for row in morph_score["rows"] if not row["hit"]]},
        "live_db": {"rows_before": pre_count, "rows_after": post_count, "unchanged": True},
    }
    runner.write_json(args.out / "results.json", result)
    print(json.dumps({
        "taste": {k: result["taste"][k] for k in ("rules", "categories", "hit_at_1", "hit_at_5", "hit_at_5_pct", "misses")},
        "morph": result["morph"], "live_db": result["live_db"],
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
