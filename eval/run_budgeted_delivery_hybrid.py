"""Evaluate an 800-token Adapt core digest plus scoped MemRight retrieval (arm E)."""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
from pathlib import Path


ROOT = Path(r"D:\Claude")
HERE = Path(__file__).resolve().parent
PRIMARY = ROOT / ".cache/adapt-delivery-parity/full"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/budgeted-deepseek"


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "budgeted_delivery_runner")


def core_treatment(treatment: dict) -> tuple[dict, str, int]:
    records = [row for row in treatment["records"] if row.get("source_status") == "baseline"]
    if len(records) != 13:
        raise RuntimeError(f"expected 13 inherited core rules, found {len(records)}")
    core = {**treatment, "variant": "baseline-core-13", "records": records}
    block = runner.adapt_block(core)
    tokens = math.ceil(len(block) / 4)
    if tokens > 800:
        raise RuntimeError(f"core digest exceeds 800-token estimate: {tokens}")
    return core, block, tokens


def combine_answers(primary: dict, arm_e: dict, target_arm: str = "E") -> dict:
    answers = [row for row in primary["answers"] if row["arm"] in {"A", "T", "D"}]
    answers.extend(arm_e["answers"])
    expected = {(arm, row["case_id"]) for arm in ("A", "T", "D", target_arm)
                for row in primary["answers"] if row["arm"] == "A"}
    actual = {(row["arm"], row["case_id"]) for row in answers}
    if expected != actual or len(answers) != 240:
        raise RuntimeError("budgeted combined actor coverage mismatch")
    return {"input_sha256": runner.sha_json(answers), "answers": answers}


def analyze(cases: list[dict], actor: dict, grades: dict, core_chars: int,
            core_tokens: int, out: Path, target_arm: str = "E",
            core_records: int = 13) -> dict:
    case_by_id = {row["case_id"]: row for row in cases}
    audited_controls = runner.control_audit.audit(actor)
    audited_by_key = {(row["arm"], row["case_id"]): row
                      for row in audited_controls["rows"]}
    runner.write_json(out / "control-audit.json", audited_controls)
    metrics = {}
    present = {row["arm"] for row in grades["grades"]}
    arms = [arm for arm in ("A", "T", "D", target_arm) if arm in present]
    for arm in arms:
        rows = [row for row in grades["grades"] if row["arm"] == arm]
        positives = [row for row in rows if case_by_id[row["case_id"]]["cohort"] != "control"]
        controls = [row for row in rows if case_by_id[row["case_id"]]["cohort"] == "control"]
        metrics[arm] = {
            "adherence_pct": round(100 * runner._mean([r["adherence"] / 2 for r in positives]), 1),
            "full_adherence_pct": round(100 * runner._mean([r["adherence"] == 2 for r in positives]), 1),
            "task_correct_pct": round(100 * runner._mean([r["task_correct"] / 2 for r in rows]), 1),
            "control_intrusion_pct": round(100 * runner._mean([
                audited_by_key[(r["arm"], r["case_id"])]["intrusion"]
                for r in controls]), 1),
            "grader_control_intrusion_pct": round(
                100 * runner._mean([bool(r["intrusion"]) for r in controls]), 1),
        }
    pairs = [("A", target_arm), ("T", target_arm)]
    if "D" in present:
        pairs.append(("D", target_arm))
    comparisons = {f"{left}_vs_{right}": runner.paired_arm_stats(
        grades["grades"], cases, left, right) for left, right in pairs}
    e, a, t = metrics[target_arm], metrics["A"], metrics["T"]
    result = {
        "grader": grades["grader"], "case_count": len(cases),
        "control_intrusion_method": audited_controls["method"],
        "core": {"records": core_records, "chars": core_chars,
                 "estimated_tokens": core_tokens},
        "metrics": metrics, "paired_comparisons": comparisons,
        "taste_parity": (e["adherence_pct"] >= t["adherence_pct"] - 5 and
                         e["control_intrusion_pct"] <= t["control_intrusion_pct"] + 5),
        "production_gate_without_power_requirement": (
            e["adherence_pct"] - a["adherence_pct"] >= 15 and
            e["task_correct_pct"] >= a["task_correct_pct"] - 5 and
            e["control_intrusion_pct"] <= 5 and core_tokens <= 800),
        "underpowered_30_discordant": (
            comparisons[f"A_vs_{target_arm}"]["full_adherence_discordance"]["discordant"] < 30),
    }
    runner.write_json(out / "analysis.json", result)
    lines = ["# Budgeted Adapt hybrid", "",
             f"- grader: {grades['grader']}",
             f"- core: 13 rules, {core_chars} chars, ~{core_tokens} tokens", "",
             "| Arm | Adherence | Full | Correct | Intrusion |",
             "|---|---:|---:|---:|---:|"]
    for arm in arms:
        row = metrics[arm]
        lines.append(f"| {arm} | {row['adherence_pct']:.1f}% | {row['full_adherence_pct']:.1f}% | "
                     f"{row['task_correct_pct']:.1f}% | {row['control_intrusion_pct']:.1f}% |")
    lines += ["", f"Taste parity: **{'PASS' if result['taste_parity'] else 'FAIL'}**",
              f"800-token production gate (power aside): **{'PASS' if result['production_gate_without_power_requirement'] else 'FAIL'}**",
              f"30-discordant power requirement: **{'UNDERPOWERED' if result['underpowered_30_discordant'] else 'MET'}**"]
    (out / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", choices=("actor", "grade", "analyze", "all"), default="all")
    parser.add_argument("--grader-model", default=runner.DEFAULT_GRADER_MODEL)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--core-file", type=Path, default=None)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    value_set = runner.read_json(PRIMARY / "frozen/value-set.json")
    treatment = runner.read_json(PRIMARY / "frozen/adapt-treatment.json")
    retrieval = runner.read_json(PRIMARY / "retrieval.json")
    primary_actor = runner.read_json(PRIMARY / "actor-results.json")
    cases = value_set["cases"]
    if args.core_file:
        compiled = runner.read_json(args.core_file)
        core = {**treatment, "variant": "compiled-core",
                "records": compiled["rules"]}
        block, tokens, target_arm = compiled["policy_block"], int(compiled["estimated_tokens"]), "F"
        core_record_count = len(compiled["rules"])
        if tokens > 800:
            raise RuntimeError("compiled core exceeds 800 tokens")
    else:
        core, block, tokens = core_treatment(treatment)
        target_arm = "E"
        core_record_count = len(core["records"])
    runner.write_json(args.out / "core-treatment.json", core)
    combined_path = args.out / "actor-results.json"
    grader_path = args.out / "grader-results.json"
    combined = grades = None
    if args.stage in {"actor", "all"}:
        arm_e = runner.run_actor(
            value_set, treatment, cases, retrieval, args.out / "actor-e", args.resume,
            arm_specs={target_arm: {"policy": block, "source": "B"}})
        combined = combine_answers(primary_actor, arm_e, target_arm)
        runner.write_json(combined_path, combined)
    elif combined_path.exists():
        combined = runner.read_json(combined_path)
    if args.stage in {"grade", "all"}:
        if combined is None:
            raise RuntimeError("actor-results.json required")
        grades = runner.run_grader(value_set, cases, combined, args.out, args.resume,
                                   grader_model=args.grader_model)
    elif grader_path.exists():
        grades = runner.read_json(grader_path)
    if args.stage in {"analyze", "all"}:
        if grades is None:
            raise RuntimeError("grader-results.json required")
        print(json.dumps(analyze(cases, combined, grades, len(block), tokens, args.out,
                                 target_arm, core_record_count), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
