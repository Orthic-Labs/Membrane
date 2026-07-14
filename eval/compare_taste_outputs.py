#!/usr/bin/env python3
"""Compare Adapt compiler rules with captured CommandCode taste digests."""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Sequence

import numpy as np
from scipy.optimize import linear_sum_assignment


# Workspace root derived from this file's location — a hardcoded D:/Claude
# broke every non-Windows checkout (eval → adapt → memory → pipelines → tools → root).
WS = Path(__file__).resolve().parents[5]
REPLAY_DIR = WS / "tools/pipelines/memory/replay"
sys.path.insert(0, str(REPLAY_DIR))
from chunk_tournament import ExactEmbeddingGemma


_TASTE_RULE_RE = re.compile(r"^-\s+(.+?)\s+Confidence:\s+[0-9.]+\s*$")


def parse_taste_rules(path: Path) -> list[str]:
    rules = []
    for line in path.read_text(encoding="utf-8").splitlines():
        match = _TASTE_RULE_RE.match(line.strip())
        if match:
            rules.append(match.group(1).strip())
    return rules


def load_adapt_rules(path: Path, *, status: str | None = None,
                     record_type: str | None = None) -> list[str]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    rules = payload.get("rules")
    if not isinstance(rules, list):
        raise ValueError("Adapt results must contain a rules list")
    return [
        item["rule"] for item in rules
        if isinstance(item, dict)
        and isinstance(item.get("rule"), str)
        and (status is None or item.get("status") == status)
        and (record_type is None or item.get("record_type") == record_type)
    ]


def without_exact(rules: Sequence[str], excluded: Sequence[str]) -> list[str]:
    excluded_set = set(excluded)
    return [rule for rule in rules if rule not in excluded_set]


def compare_vectors(a: Sequence[str], b: Sequence[str], embedder) -> dict:
    if not a or not b:
        raise ValueError("both rule sets must be non-empty")
    a_vectors = embedder.embed_documents(list(a))
    b_vectors = embedder.embed_documents(list(b))
    similarity = a_vectors @ b_vectors.T
    rows, columns = linear_sum_assignment(-similarity)
    assigned = similarity[rows, columns]
    pairs = sorted((
        {"a_index": int(row), "b_index": int(column),
         "similarity": float(similarity[row, column]),
         "a_rule": a[row], "b_rule": b[column]}
        for row, column in zip(rows, columns)
    ), key=lambda item: item["similarity"], reverse=True)
    return {
        "a": len(a),
        "b": len(b),
        "matched": len(assigned),
        "one_to_one_mean": float(np.mean(assigned)),
        "one_to_one_median": float(np.median(assigned)),
        "pairs_ge_0.8": int(np.sum(assigned >= 0.8)),
        "pairs_ge_0.7": int(np.sum(assigned >= 0.7)),
        "pairs_ge_0.6": int(np.sum(assigned >= 0.6)),
        "a_best_mean": float(np.mean(np.max(similarity, axis=1))),
        "b_best_mean": float(np.mean(np.max(similarity, axis=0))),
        "pairs": pairs,
    }


def internal_pairs(rules: Sequence[str], embedder, *, threshold: float = 0.7) -> list[dict]:
    if len(rules) < 2:
        return []
    vectors = embedder.embed_documents(list(rules))
    similarity = vectors @ vectors.T
    pairs = []
    for left in range(len(rules)):
        for right in range(left + 1, len(rules)):
            score = float(similarity[left, right])
            if score >= threshold:
                pairs.append({
                    "left_index": left,
                    "right_index": right,
                    "similarity": score,
                    "left_rule": rules[left],
                    "right_rule": rules[right],
                })
    return sorted(pairs, key=lambda item: item["similarity"], reverse=True)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapt-results", type=Path, required=True)
    parser.add_argument("--m3-taste", type=Path, required=True)
    parser.add_argument("--glm-taste", type=Path, required=True)
    parser.add_argument("--baseline-taste", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    adapt_all = load_adapt_rules(args.adapt_results)
    adapt_candidate = load_adapt_rules(args.adapt_results, status="candidate")
    adapt_standing = load_adapt_rules(
        args.adapt_results, status="candidate", record_type="standing_preference"
    )
    m3 = parse_taste_rules(args.m3_taste)
    glm = parse_taste_rules(args.glm_taste)
    embedder = ExactEmbeddingGemma()
    comparisons = {
        "adapt_all_vs_m3": compare_vectors(adapt_all, m3, embedder),
        "adapt_candidate_vs_m3": compare_vectors(adapt_candidate, m3, embedder),
        "adapt_standing_candidate_vs_m3": compare_vectors(adapt_standing, m3, embedder),
        "adapt_all_vs_glm": compare_vectors(adapt_all, glm, embedder),
        "m3_vs_glm": compare_vectors(m3, glm, embedder),
    }
    if args.baseline_taste:
        baseline = parse_taste_rules(args.baseline_taste)
        adapt_new = without_exact(adapt_all, baseline)
        adapt_candidate_new = without_exact(adapt_candidate, baseline)
        m3_new = without_exact(m3, baseline)
        glm_new = without_exact(glm, baseline)
        comparisons.update({
            "adapt_new_vs_m3_new": compare_vectors(adapt_new, m3_new, embedder),
            "adapt_candidate_new_vs_m3_new": compare_vectors(
                adapt_candidate_new, m3_new, embedder
            ),
            "adapt_new_vs_glm_new": compare_vectors(adapt_new, glm_new, embedder),
            "m3_new_vs_glm_new": compare_vectors(m3_new, glm_new, embedder),
        })
    report = {
        "model": embedder.pipeline_key,
        "baseline_rule_count": len(baseline) if args.baseline_taste else None,
        "coherence": {
            "adapt_all_pairs_ge_0.7": internal_pairs(adapt_all, embedder),
            "adapt_candidate_pairs_ge_0.7": internal_pairs(adapt_candidate, embedder),
        },
        "comparisons": comparisons,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({
        "model": report["model"],
        "out": str(args.out),
        "metrics": {
            name: {key: value for key, value in result.items() if key != "pairs"}
            for name, result in comparisons.items()
        },
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
