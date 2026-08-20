"""Gate 0 — retrieval-grader for the 4-arm A/B harness.

Consumes ``run_value_ab.py``'s ``results.json`` and emits per-arm retrieval
metrics + paired comparisons. The grader is intentionally deterministic
(no LLM calls) so the retrieval shape can be verified before any LLM
adherence grading is wired.

Metrics computed per arm:

  - hit_at_K              fraction of positive prompts where expected_id appears
                          in the top-K retrieved ids
  - mrr_at_K              mean reciprocal rank of expected_id (0 if absent)
  - intrusion_at_K        fraction of control prompts that pulled a high-confidence
                          rule into the top-K
  - wall_avg_ms / wall_p95_ms

Paired tests across arms:

  - hit_at_K   delta with McNemar-style exact test (discordant counts)
  - intrusion  delta with McNemar
  - wall      delta with bootstrap CI

Wilson intervals are produced for each rate. Raw counts and the discordant
matrix are persisted in ``graded.json`` so downstream reports can quote them.

Usage::

    py -3.11 membrane/adapt/eval/grade.py \\
        --results /path/to/results.json \\
        --out /path/to/graded/

Outputs::

    <out>/graded.json   structured metric + paired-test results
    <out>/grade.md      human-readable summary table
"""
from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from collections import Counter
from pathlib import Path
from typing import Iterable


# ----- Wilson interval (95%) -----

def wilson_interval(p: float, n: int, z: float = 1.96) -> tuple[float, float]:
    """Asymptotic 95% Wilson interval for a Bernoulli proportion."""
    if n == 0:
        return (0.0, 0.0)
    denom = 1 + z * z / n
    center = (p + z * z / (2 * n)) / denom
    half = z * math.sqrt((p * (1 - p) + z * z / (4 * n)) / n) / denom
    return (max(0.0, center - half), min(1.0, center + half))


# ----- Per-arm metrics -----

def _mrr(ranked: list[str], expected: str | None, k: int) -> float:
    if not expected:
        return 0.0
    for i, rid in enumerate(ranked[:k], 1):
        if rid == expected:
            return 1.0 / i
    return 0.0


def _hit(ranked: list[str], expected: str | None, k: int) -> int:
    return 1 if expected in ranked[:k] else 0


def _intrusion(ranked: list[str], expected: str | None,
               rule_ids: set[str], k: int) -> int:
    """A control prompt that pulls ANY high-confidence rule id is intrusion."""
    if expected is not None:
        return 0  # only counts for controls
    return 1 if any(rid in rule_ids for rid in ranked[:k]) else 0


def _percentile(values: list[int | float], p: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    k = (len(s) - 1) * p
    f = math.floor(k)
    c = math.ceil(k)
    if f == c:
        return float(s[int(k)])
    return float(s[f] * (c - k) + s[c] * (k - f))


def arm_metrics(arm: dict, query_rows: list[dict], k: int = 10) -> dict:
    """Compute retrieval metrics for one arm against the labeled query set.

    For arm C, ``static_block`` is scored as a separate text-overlap metric:
    a positive query is considered ``hit`` if any block line is a substring of
    any retrieved row's text (the harness records the block but does not
    inject it; this grades whether the static policy WOULD have applied).
    """
    rule_ids = {qr["expected_id"] for qr in query_rows if qr["expected_id"]}
    positives = [qr for qr in query_rows if qr["kind"] == "positive"]
    controls = [qr for qr in query_rows if qr["kind"] == "control"]
    static_block = arm.get("static_block", "")
    block_lines = [ln.strip() for ln in static_block.splitlines()
                  if ln.strip()]
    hits = []
    mrrs = []
    intrusions = []
    walls = []
    per_query = []
    for qr in query_rows:
        rid = qr["row_id"]
        ranked = arm["ranked"].get(rid, [])
        ranked_texts = arm.get("ranked_texts", {}).get(rid, [])
        if arm["arm"] == "C" and block_lines and ranked_texts:
            hit = 1 if any(any(bl in txt for txt in ranked_texts)
                           for bl in block_lines) else 0
        else:
            hit = _hit(ranked, qr["expected_id"], k)
        if arm["arm"] == "C" and block_lines and ranked_texts:
            mrr = 0.0
            for i, txt in enumerate(ranked_texts[:k], 1):
                if any(bl in txt for bl in block_lines):
                    mrr = 1.0 / i
                    break
        else:
            mrr = _mrr(ranked, qr["expected_id"], k)
        intr = _intrusion(ranked, qr["expected_id"], rule_ids, k)
        wall = arm["wall_ms"].get(rid, 0)
        hits.append(hit)
        mrrs.append(mrr)
        intrusions.append(intr)
        walls.append(wall)
        per_query.append({
            "row_id": rid, "kind": qr["kind"],
            "expected_id": qr["expected_id"],
            "ranked_ids": ranked, "hit": hit, "mrr": mrr,
            "intrusion": intr, "wall_ms": wall,
        })
    n_pos = len(positives)
    n_ctrl = len(controls)
    hit_rate = sum(hits[:n_pos]) / n_pos if n_pos else 0.0
    mrr_avg = sum(mrrs[:n_pos]) / n_pos if n_pos else 0.0
    intr_rate = sum(intrusions[n_pos:]) / n_ctrl if n_ctrl else 0.0
    return {
        "arm": arm["arm"],
        "n_pos": n_pos, "n_ctrl": n_ctrl,
        "hit_at_K": hit_rate,
        "mrr_at_K": mrr_avg,
        "intrusion_at_K": intr_rate,
        "wall_avg_ms": statistics.mean(walls) if walls else 0.0,
        "wall_p95_ms": _percentile(walls, 0.95),
        "hit_wilson": list(wilson_interval(hit_rate, n_pos)),
        "intrusion_wilson": list(wilson_interval(intr_rate, n_ctrl)),
        "static_block_lines": len(block_lines),
        "per_query": per_query,
    }


# ----- Paired tests -----

def mcnemar_discordant(a_hits: list[int], b_hits: list[int]) -> dict:
    """Exact McNemar counts for paired binary outcomes.

    discordant = (a_hit, b_miss), (a_miss, b_hit).
    With small N we report raw counts; the caller decides whether the
    discordant total >= 30 (the v2 plan's power threshold) before quoting a
    p-value.
    """
    if len(a_hits) != len(b_hits):
        raise ValueError("paired vectors must be equal length")
    a_only = sum(1 for a, b in zip(a_hits, b_hits) if a == 1 and b == 0)
    b_only = sum(1 for a, b in zip(a_hits, b_hits) if a == 0 and b == 1)
    return {
        "a_only_hits_b_misses": a_only,
        "b_only_hits_a_misses": b_only,
        "discordant_total": a_only + b_only,
        "underpowered": (a_only + b_only) < 30,
    }


def paired_compare(a: dict, b: dict, *, k: int = 10) -> dict:
    """Pair A vs B on hit_at_K and intrusion_at_K using McNemar discordant counts."""
    a_pos_hits = [pq["hit"] for pq in a["per_query"]
                  if pq["kind"] == "positive"]
    b_pos_hits = [pq["hit"] for pq in b["per_query"]
                  if pq["kind"] == "positive"]
    a_ctrl_intr = [pq["intrusion"] for pq in a["per_query"]
                   if pq["kind"] == "control"]
    b_ctrl_intr = [pq["intrusion"] for pq in b["per_query"]
                   if pq["kind"] == "control"]
    a_walls = [pq["wall_ms"] for pq in a["per_query"]]
    b_walls = [pq["wall_ms"] for pq in b["per_query"]]
    return {
        "vs": f"{a['arm']}->{b['arm']}",
        "hit_delta": b["hit_at_K"] - a["hit_at_K"],
        "intrusion_delta": b["intrusion_at_K"] - a["intrusion_at_K"],
        "wall_delta_ms_avg": b["wall_avg_ms"] - a["wall_avg_ms"],
        "hit_mcnemar": mcnemar_discordant(a_pos_hits, b_pos_hits),
        "intrusion_mcnemar": mcnemar_discordant(a_ctrl_intr, b_ctrl_intr),
        "wall_diff_samples": [b - a for a, b in zip(a_walls, b_walls)],
    }


def bootstrap_ci(diff_samples: list[float], iters: int = 1000,
                 p: float = 0.95) -> tuple[float, float]:
    """Percentile bootstrap CI on paired differences.

    Resamples with replacement so the means actually vary — the previous
    cyclic-resampling variant produced a degenerate CI of (mean, mean) on
    small N.
    """
    import random
    if not diff_samples:
        return (0.0, 0.0)
    n = len(diff_samples)
    means = []
    for _ in range(iters):
        sample = [diff_samples[random.randrange(n)] for _ in range(n)]
        means.append(sum(sample) / n)
    means.sort()
    lo = means[int((1 - p) / 2 * iters)]
    hi = means[int((1 + p) / 2 * iters) - 1]
    return (lo, hi)


# ----- Orchestration -----

def grade(results_path: Path, out_dir: Path, k: int = 10) -> int:
    raw = json.loads(results_path.read_text(encoding="utf-8"))
    arms = raw["arms"]
    rows = raw["query_rows"]

    per_arm = [arm_metrics(a, rows, k=k) for a in arms]
    paired = []
    # Pairwise against the baseline (arm A) — that's what the plan compares.
    if per_arm:
        baseline = per_arm[0]
        for other in per_arm[1:]:
            cmp = paired_compare(baseline, other, k=k)
            # Add bootstrap CI on wall diff.
            cmp["wall_bootstrap_ci"] = list(bootstrap_ci(
                cmp.pop("wall_diff_samples")))
            paired.append(cmp)

    out_dir.mkdir(parents=True, exist_ok=True)
    graded = {
        "k": k,
        "generated_at": raw.get("generated_at", ""),
        "smoke": raw.get("smoke", False),
        "rules_count": raw.get("rules_count", 0),
        "per_arm": per_arm,
        "paired_vs_baseline": paired,
    }
    (out_dir / "graded.json").write_text(
        json.dumps(graded, indent=2, ensure_ascii=False), encoding="utf-8")

    report = _format_report(graded)
    (out_dir / "grade.md").write_text(report, encoding="utf-8")
    print(f"graded: {out_dir / 'graded.json'}")
    print(f"report: {out_dir / 'grade.md'}")
    return 0


def _format_report(graded: dict) -> str:
    L = ["# Gate 0 — retrieval grader", ""]
    L.append(f"- generated: {graded['generated_at']}")
    L.append(f"- K: {graded['k']}  smoke: {graded['smoke']}")
    L.append("")
    L.append("## Per-arm metrics")
    L.append("")
    L.append("| arm | n_pos | n_ctrl | hit@K | mrr@K | intrusion@K | "
             "wall_avg_ms | wall_p95_ms | hit_wilson | intr_wilson |")
    L.append("|---|---:|---:|---:|---:|---:|---:|---:|---|---|")
    for a in graded["per_arm"]:
        hw = f"[{a['hit_wilson'][0]:.3f},{a['hit_wilson'][1]:.3f}]"
        iw = f"[{a['intrusion_wilson'][0]:.3f},{a['intrusion_wilson'][1]:.3f}]"
        L.append(f"| {a['arm']} | {a['n_pos']} | {a['n_ctrl']} | "
                 f"{a['hit_at_K']:.3f} | {a['mrr_at_K']:.3f} | "
                 f"{a['intrusion_at_K']:.3f} | {a['wall_avg_ms']:.0f} | "
                 f"{a['wall_p95_ms']:.0f} | {hw} | {iw} |")
    L.append("")
    L.append("## Paired vs baseline (arm A)")
    L.append("")
    for p in graded["paired_vs_baseline"]:
        h = p["hit_mcnemar"]
        i = p["intrusion_mcnemar"]
        under = " UNDERPOWERED" if h["underpowered"] else ""
        L.append(f"### {p['vs']}")
        L.append(f"- hit delta: {p['hit_delta']:+.3f}  "
                 f"discordant: a_only={h['a_only_hits_b_misses']} "
                 f"b_only={h['b_only_hits_a_misses']} "
                 f"total={h['discordant_total']}{under}")
        L.append(f"- intrusion delta: {p['intrusion_delta']:+.3f}  "
                 f"discordant: total={i['discordant_total']}")
        L.append(f"- wall delta avg: {p['wall_delta_ms_avg']:+.0f}ms  "
                 f"95% bootstrap CI: "
                 f"[{p['wall_bootstrap_ci'][0]:.0f},"
                 f"{p['wall_bootstrap_ci'][1]:.0f}]ms")
        L.append("")
    return "\n".join(L) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--results", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--k", type=int, default=10)
    args = ap.parse_args(argv)
    return grade(args.results, args.out, k=args.k)


if __name__ == "__main__":
    sys.exit(main())
