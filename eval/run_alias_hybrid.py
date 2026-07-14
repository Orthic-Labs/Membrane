"""Evaluate compiled Adapt core plus source-alias MemRight retrieval (arm G)."""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[5]  # workspace root — hardcoded D:\Claude broke non-Windows checkouts
HERE = Path(__file__).resolve().parent
PRIMARY = ROOT / ".cache/adapt-delivery-parity/full"
DEFAULT_ALIAS_DB = ROOT / ".cache/adapt-delivery-parity/evidence-alias/alias.db"
DEFAULT_CORE = ROOT / ".cache/adapt-delivery-parity/compiled-core/core.json"


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "alias_hybrid_runner")
budgeted = _load(HERE / "run_budgeted_delivery_hybrid.py", "alias_hybrid_budgeted")
production = _load(HERE / "smoke_production_delivery.py", "alias_hybrid_production")


def combine_answers(primary: dict, arm_g: dict, cases: list[dict]) -> dict:
    ids = {row["case_id"] for row in cases}
    answers = [row for row in primary["answers"]
               if row["arm"] in {"A", "T"} and row["case_id"] in ids]
    answers.extend(arm_g["answers"])
    expected = {(arm, case_id) for arm in ("A", "T", "G") for case_id in ids}
    actual = {(row["arm"], row["case_id"]) for row in answers}
    if actual != expected or len(answers) != len(expected):
        raise RuntimeError("alias hybrid actor coverage mismatch")
    return {"input_sha256": runner.sha_json(answers), "answers": answers}


def run_alias_retrieval(cases: list[dict], alias_db: Path, out: Path,
                        memright: Path) -> dict:
    path = out / "retrieval.json"
    input_hash = runner.sha_json({"db": str(alias_db), "cases": cases})
    before_ok, before_count, before_msg = runner.value_ab.integrity_check(alias_db)
    if not before_ok:
        raise RuntimeError(f"alias DB integrity failed: {before_msg}")
    port = runner.value_ab._free_port()
    service = runner.value_ab._start_service(memright, alias_db, port)
    try:
        runner.value_ab._wait_ready(port)
        ranked = runner.replay_all(memright, alias_db, port, cases,
                                   out / "inputs/G.jsonl")
    finally:
        runner.value_ab._stop_service(service)
    after_ok, after_count, after_msg = runner.value_ab.integrity_check(alias_db)
    if not after_ok or after_count != before_count:
        raise RuntimeError(f"alias DB changed during retrieval: {after_msg}")
    result = {"input_sha256": input_hash, "B": ranked,
              "alias_db": {"rows_before": before_count, "rows_after": after_count,
                           "unchanged": before_count == after_count}}
    runner.write_json(path, result)
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", choices=("retrieval", "actor", "grade", "analyze", "all"),
                        default="all")
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--alias-db", type=Path, default=DEFAULT_ALIAS_DB)
    parser.add_argument("--production-shape", action="store_true",
                        help="rebuild the alias DB through PreferenceRecord v1.2")
    parser.add_argument("--core-file", type=Path, default=DEFAULT_CORE)
    parser.add_argument("--memright-bin", type=Path, default=runner.DEFAULT_MEMRIGHT)
    parser.add_argument("--grader-model", default=runner.DEFAULT_GRADER_MODEL)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args(argv)
    out = args.out or ROOT / ".cache/adapt-delivery-parity" / (
        "alias-hybrid-smoke" if args.smoke else "alias-hybrid-deepseek")
    out.mkdir(parents=True, exist_ok=True)
    value_set = runner.read_json(PRIMARY / "frozen/value-set.json")
    treatment = runner.read_json(PRIMARY / "frozen/adapt-treatment.json")
    primary_actor = runner.read_json(PRIMARY / "actor-results.json")
    compiled = runner.read_json(args.core_file)
    cases = runner.select_cases(value_set["cases"], args.smoke)
    block = compiled["policy_block"]
    tokens = int(compiled["estimated_tokens"])
    if tokens > 800:
        raise RuntimeError("compiled core exceeds 800 tokens")

    alias_db = args.alias_db
    if args.production_shape:
        alias_db = out / "production-alias.db"
        build = production.build_production_alias_db(
            args.memright_bin, runner.DEFAULT_LIVE_DB, alias_db, treatment
        )
        runner.write_json(out / "production-alias-build.json", build)

    retrieval_path = out / "retrieval.json"
    actor_path = out / "actor-results.json"
    grader_path = out / "grader-results.json"
    retrieval = combined = grades = None
    if args.stage in {"retrieval", "all"}:
        retrieval = run_alias_retrieval(cases, alias_db, out, args.memright_bin)
    elif retrieval_path.exists():
        retrieval = runner.read_json(retrieval_path)
    if args.stage in {"actor", "all"}:
        if retrieval is None:
            raise RuntimeError("retrieval.json required")
        arm_g = runner.run_actor(
            value_set, treatment, cases, retrieval, out / "actor-g", args.resume,
            arm_specs={"G": {"policy": block, "source": "B"}})
        combined = combine_answers(primary_actor, arm_g, cases)
        runner.write_json(actor_path, combined)
    elif actor_path.exists():
        combined = runner.read_json(actor_path)
    if args.stage in {"grade", "all"}:
        if combined is None:
            raise RuntimeError("actor-results.json required")
        grades = runner.run_grader(value_set, cases, combined, out, args.resume,
                                   grader_model=args.grader_model)
    elif grader_path.exists():
        grades = runner.read_json(grader_path)
    if args.stage in {"analyze", "all"}:
        if combined is None or grades is None:
            raise RuntimeError("actor and grader results required")
        result = budgeted.analyze(cases, combined, grades, len(block), tokens, out,
                                  "G", len(compiled["rules"]))
        print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
