"""Compare two blind graders over identical delivery-experiment answers."""
from __future__ import annotations

import argparse
import json
from pathlib import Path


ROOT = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
DEFAULT_A = ROOT / ".cache/morph-delivery-parity/full/grader-results.json"
DEFAULT_B = ROOT / ".cache/morph-delivery-parity/full-nemotron/grader-results.json"
DEFAULT_OUT = ROOT / ".cache/morph-delivery-parity/grader-agreement.json"


def weighted_kappa(left: list[int], right: list[int]) -> float:
    if len(left) != len(right) or not left:
        raise ValueError("paired non-empty score vectors required")
    n = len(left)
    observed = sum(abs(a - b) / 2 for a, b in zip(left, right)) / n
    lp = [left.count(i) / n for i in range(3)]
    rp = [right.count(i) / n for i in range(3)]
    expected = sum(lp[i] * rp[j] * abs(i - j) / 2
                   for i in range(3) for j in range(3))
    return 1.0 if expected == 0 and observed == 0 else round(1 - observed / expected, 3)


def compare(a: dict, b: dict) -> dict:
    key = lambda row: (row["arm"], row["case_id"])
    left, right = {key(r): r for r in a["grades"]}, {key(r): r for r in b["grades"]}
    if set(left) != set(right):
        raise ValueError("grader coverage differs")
    keys = sorted(left)
    adherence_a = [left[k]["adherence"] for k in keys]
    adherence_b = [right[k]["adherence"] for k in keys]
    correct_a = [left[k]["task_correct"] for k in keys]
    correct_b = [right[k]["task_correct"] for k in keys]
    intrusion_equal = sum(left[k]["intrusion"] == right[k]["intrusion"] for k in keys)
    return {
        "grader_a": a["grader"], "grader_b": b["grader"], "paired_rows": len(keys),
        "adherence_exact_agreement_pct": round(100 * sum(x == y for x, y in
                                                         zip(adherence_a, adherence_b)) / len(keys), 1),
        "adherence_linear_weighted_kappa": weighted_kappa(adherence_a, adherence_b),
        "task_correct_exact_agreement_pct": round(100 * sum(x == y for x, y in
                                                            zip(correct_a, correct_b)) / len(keys), 1),
        "task_correct_linear_weighted_kappa": weighted_kappa(correct_a, correct_b),
        "intrusion_agreement_pct": round(100 * intrusion_equal / len(keys), 1),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, default=DEFAULT_A)
    parser.add_argument("--b", type=Path, default=DEFAULT_B)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)
    result = compare(json.loads(args.a.read_text(encoding="utf-8")),
                     json.loads(args.b.read_text(encoding="utf-8")))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
