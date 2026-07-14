"""Measure source-evidence recall after adding bounded trigger aliases to records."""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[5]  # workspace root — hardcoded D:\Claude broke non-Windows checkouts
HERE = Path(__file__).resolve().parent
DEFAULT_TREATMENT = ROOT / ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json"
DEFAULT_BASELINE = ROOT / ".cache/adapt-delivery-parity/full/evidence-recall/results.json"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/evidence-alias"
MAX_ALIAS_CHARS = 320


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "alias_retrieval_runner")
recall = _load(HERE / "evaluate_delivery_evidence_recall.py", "alias_recall_scorer")


def enrich_records(records: list[dict], max_chars: int = MAX_ALIAS_CHARS) -> list[dict]:
    enriched = []
    for source in records:
        row = dict(source)
        support = row.get("support") or []
        if isinstance(support, dict):
            support = [support]
        aliases = []
        used = 0
        for item in support:
            text = " ".join(str(item.get("evidence", "")).split())
            if not text or text.casefold() in {value.casefold() for value in aliases}:
                continue
            remaining = max_chars - used
            if remaining <= 0:
                break
            aliases.append(text[:remaining])
            used += len(aliases[-1])
        row["retrieval_aliases"] = aliases
        if aliases:
            row["rule"] = f"{row['rule']} Trigger phrases: {' | '.join(aliases)}"
        enriched.append(row)
    return enriched


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--treatment", type=Path, default=DEFAULT_TREATMENT)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--live-db", type=Path, default=runner.DEFAULT_LIVE_DB)
    parser.add_argument("--memright-bin", type=Path, default=runner.DEFAULT_MEMRIGHT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--limit", type=int, default=None)
    args = parser.parse_args(argv)
    treatment = json.loads(args.treatment.read_text(encoding="utf-8"))
    source_records = treatment["records"][:args.limit] if args.limit else treatment["records"]
    records = enrich_records(source_records)
    cases = recall.build_cases(source_records)
    args.out.mkdir(parents=True, exist_ok=True)
    pre_ok, pre_count, pre_msg = runner.value_ab.integrity_check(args.live_db)
    if not pre_ok:
        raise RuntimeError(f"live DB integrity failed before experiment: {pre_msg}")
    ranked, integrity = runner._run_replay_db(
        args.memright_bin, args.live_db, args.out / "alias.db", cases,
        args.out / "queries.jsonl", records)
    post_ok, post_count, post_msg = runner.value_ab.integrity_check(args.live_db)
    if not post_ok or post_count != pre_count:
        raise RuntimeError(f"live DB changed during alias experiment: {post_msg}")
    result = {
        "record_count": len(records),
        "aliased_record_count": sum(bool(row["retrieval_aliases"]) for row in records),
        "alias_char_budget": MAX_ALIAS_CHARS,
        "integrity": integrity,
        "live_db": {"rows_before": pre_count, "rows_after": post_count,
                    "unchanged": pre_count == post_count},
        **recall.score_cases(cases, ranked),
    }
    if args.baseline.exists() and not args.limit:
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
        old = baseline["metrics"]["source_evidence"]
        new = result["metrics"]["source_evidence"]
        result["delta"] = {
            "source_hit_at_5_pp": round(new["hit_at_5_pct"] - old["hit_at_5_pct"], 1),
            "source_mrr_at_5": round(new["mrr_at_5"] - old["mrr_at_5"], 3),
        }
    runner.write_json(args.out / "enriched-treatment.json", {
        **treatment, "variant": "source-evidence-alias-v1", "records": records,
    })
    runner.write_json(args.out / "results.json", result)
    print(json.dumps({"metrics": result["metrics"], "delta": result.get("delta")}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
