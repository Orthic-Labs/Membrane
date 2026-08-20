"""Exhaustively test whether Adapt records recall from their own source evidence."""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from collections import defaultdict
from pathlib import Path


ROOT = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
HERE = Path(__file__).resolve().parent
DEFAULT_TREATMENT = ROOT / ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json"
DEFAULT_DB = ROOT / ".cache/adapt-delivery-parity/full/B.db"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/full/evidence-recall"


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "evidence_recall_runner")


def build_cases(records: list[dict]) -> list[dict]:
    cases = []
    for record in records:
        base = {"record_id": record["id"], "expected_id": f"{record['scope']}/{record['id']}",
                "scope": record["scope"], "category": record["category"],
                "record_type": record["record_type"]}
        cases.append({**base, "case_id": f"self-{record['id']}",
                      "kind": "rule_self", "prompt": record["rule"]})
        support = record.get("support") or []
        if isinstance(support, dict):
            support = [support]
        for index, item in enumerate(support, 1):
            evidence = str(item.get("evidence", "")).strip()
            if evidence:
                cases.append({**base, "case_id": f"evidence-{record['id']}-{index}",
                              "kind": "source_evidence", "prompt": evidence})
    return cases


def score_cases(cases: list[dict], ranked: dict[str, dict], k: int = 5) -> dict:
    rows = []
    for case in cases:
        ids = ranked[case["case_id"]]["ranked_ids"][:k]
        try:
            rank = ids.index(case["expected_id"]) + 1
        except ValueError:
            rank = 0
        rows.append({**case, "rank": rank, "hit": bool(rank), "ranked_ids": ids})
    metrics = {}
    for kind in ("rule_self", "source_evidence"):
        subset = [row for row in rows if row["kind"] == kind]
        metrics[kind] = {
            "cases": len(subset), "hit_at_1": sum(row["rank"] == 1 for row in subset),
            "hit_at_5": sum(row["hit"] for row in subset),
            "hit_at_5_pct": round(100 * sum(row["hit"] for row in subset) / len(subset), 1),
            "mrr_at_5": round(sum((1 / row["rank"]) if row["rank"] else 0
                                  for row in subset) / len(subset), 3),
        }
    by_type = defaultdict(lambda: {"cases": 0, "hits": 0})
    for row in rows:
        if row["kind"] == "source_evidence":
            by_type[row["record_type"]]["cases"] += 1
            by_type[row["record_type"]]["hits"] += int(row["hit"])
    metrics["source_evidence_by_record_type"] = {
        key: {**value, "hit_at_5_pct": round(100 * value["hits"] / value["cases"], 1)}
        for key, value in sorted(by_type.items())
    }
    return {"metrics": metrics, "rows": rows}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--treatment", type=Path, default=DEFAULT_TREATMENT)
    parser.add_argument("--source-db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--crypt-bin", type=Path, default=runner.DEFAULT_CRYPT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)
    treatment = json.loads(args.treatment.read_text(encoding="utf-8"))
    records = treatment["records"]
    expected_count = int(treatment.get("selected_record_count", len(records)))
    if len(records) != expected_count:
        raise RuntimeError(f"treatment count mismatch: expected {expected_count}, found {len(records)}")
    cases = build_cases(records)
    args.out.mkdir(parents=True, exist_ok=True)
    eval_db = args.out / "evidence-recall.db"
    runner.value_ab.snapshot_live_db(args.source_db, eval_db)
    port = runner.value_ab._free_port()
    service = runner.value_ab._start_service(args.crypt_bin, eval_db, port)
    try:
        runner.value_ab._wait_ready(port)
        ranked = runner.replay_all(
            args.crypt_bin, eval_db, port, cases, args.out / "queries.jsonl")
    finally:
        runner.value_ab._stop_service(service)
    result = {"record_count": len(records), "case_count": len(cases),
              **score_cases(cases, ranked)}
    runner.write_json(args.out / "results.json", result)
    m = result["metrics"]
    lines = ["# Exhaustive Adapt source-evidence recall", "",
             f"- selected records: {len(records)}", f"- variant: {treatment.get('variant', 'raw')}",
             f"- total queries: {len(cases)}", "",
             "| Query kind | Cases | Hit@1 | Hit@5 | Hit@5 % | MRR@5 |",
             "|---|---:|---:|---:|---:|---:|"]
    for kind in ("rule_self", "source_evidence"):
        row = m[kind]
        lines.append(f"| {kind} | {row['cases']} | {row['hit_at_1']} | "
                     f"{row['hit_at_5']} | {row['hit_at_5_pct']:.1f}% | {row['mrr_at_5']:.3f} |")
    (args.out / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(json.dumps(m, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
