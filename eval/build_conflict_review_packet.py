#!/usr/bin/env python3
"""Build compact, coverage-checkable semantic conflict review packets."""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Sequence

import numpy as np


def authority_chunks(authority_root: Path, *, max_chars: int = 900) -> list[dict]:
    chunks: list[dict] = []
    for path in sorted(authority_root.rglob("*.md")):
        relative = path.relative_to(authority_root).as_posix()
        blocks = re.split(r"\n\s*\n", path.read_text(encoding="utf-8", errors="replace"))
        current = ""
        for block in (item.strip() for item in blocks if item.strip()):
            pieces = [block[index:index + max_chars] for index in range(0, len(block), max_chars)]
            for piece in pieces:
                if current and len(current) + len(piece) + 2 > max_chars:
                    chunks.append({"source": relative, "excerpt": current})
                    current = ""
                current = f"{current}\n\n{piece}".strip()
        if current:
            chunks.append({"source": relative, "excerpt": current})
    if not chunks:
        raise ValueError("authority root contains no Markdown content")
    return chunks


def retrieve_authority(
    rules: Sequence[dict], chunks: Sequence[dict], embedder, *, top_k: int,
    rule_vectors=None, chunk_vectors=None,
) -> dict[str, list[dict]]:
    if top_k < 1:
        raise ValueError("top_k must be positive")
    if rule_vectors is None:
        rule_vectors = embedder.embed_documents([item["rule"] for item in rules])
    if chunk_vectors is None:
        chunk_vectors = embedder.embed_documents([item["excerpt"] for item in chunks])
    similarity = rule_vectors @ chunk_vectors.T
    result: dict[str, list[dict]] = {}
    for index, rule in enumerate(rules):
        ranked = np.argsort(-similarity[index])[:top_k]
        result[rule["name"]] = [
            {
                **chunks[int(chunk_index)],
                "similarity": round(float(similarity[index, chunk_index]), 6),
            }
            for chunk_index in ranked
        ]
    return result


def near_rule_pairs(
    rules: Sequence[dict], embedder, *, threshold: float = 0.7, vectors=None
) -> dict[str, list[dict]]:
    if vectors is None:
        vectors = embedder.embed_documents([item["rule"] for item in rules])
    similarity = vectors @ vectors.T
    result = {item["name"]: [] for item in rules}
    for left in range(len(rules)):
        for right in range(left + 1, len(rules)):
            score = float(similarity[left, right])
            if score < threshold:
                continue
            left_item, right_item = rules[left], rules[right]
            result[left_item["name"]].append({
                "rule_name": right_item["name"], "rule": right_item["rule"],
                "similarity": round(score, 6),
            })
            result[right_item["name"]].append({
                "rule_name": left_item["name"], "rule": left_item["rule"],
                "similarity": round(score, 6),
            })
    return result


def _packet(batch_id: str, batch: Sequence[dict], all_rules: Sequence[dict],
            retrieved: dict[str, list[dict]], near_pairs: dict[str, list[dict]]) -> str:
    expected_names = [item["name"] for item in batch]
    detailed = []
    for item in batch:
        detailed.append({
            key: item.get(key) for key in (
                "name", "record_type", "category", "scope", "rule", "confidence", "support"
            )
        } | {
            "retrieved_authority": retrieved[item["name"]],
            "near_candidate_rules": near_pairs[item["name"]],
        })
    index = [{"name": item["name"], "scope": item.get("scope"), "rule": item["rule"]}
             for item in all_rules]
    return f"""TASK: Review one batch of compiled Morph candidates for semantic safety and coherence.

Authority excerpts outrank every candidate. Review EVERY batch candidate. The global rule index is
context for duplicates/contradictions only; do not count index-only rules as reviewed. Flag only:
authority conflict, permission expansion, overbroad scope, duplicate, contradiction, transient detail,
or evidence that does not support the claimed durable rule. Retrieved authority is candidate evidence,
not proof of conflict. Findings must cite an exact source excerpt or paired rule name.

Return JSON only:
{{"batch_id":"{batch_id}","reviews":[{{"rule_name":"exact expected name","verdict":"safe|flagged","findings":[{{"severity":"high|medium|low","kind":"authority-conflict|permission-expansion|overbroad-scope|duplicate|contradiction|transient|unsupported","evidence":"exact source/excerpt or paired rule","reason":"brief"}}]}}],"reviewed_rule_count":{len(batch)}}}

There must be exactly one review for each expected name, no extras, and safe reviews use findings=[].
EXPECTED NAMES:
{json.dumps(expected_names, ensure_ascii=False)}

BATCH CANDIDATES:
{json.dumps(detailed, ensure_ascii=False, indent=2)}

GLOBAL CANDIDATE RULE INDEX:
{json.dumps(index, ensure_ascii=False, indent=2)}
"""


def build_packets(results_path: Path, authority_root: Path, embedder, *, top_k: int = 2,
                  batch_size: int = 15, limit: int | None = None,
                  names: set[str] | None = None) -> list[dict]:
    results = json.loads(results_path.read_text(encoding="utf-8"))
    rules = [item for item in results.get("rules", [])
             if isinstance(item, dict) and item.get("status") == "candidate"
             and isinstance(item.get("name"), str) and isinstance(item.get("rule"), str)]
    if names is not None:
        rules = [item for item in rules if item["name"] in names]
        missing = names - {item["name"] for item in rules}
        if missing:
            raise ValueError(f"requested candidate names missing: {sorted(missing)}")
    if limit is not None:
        rules = rules[:limit]
    if not rules:
        raise ValueError("results contain no candidate rules")
    chunks = authority_chunks(authority_root)
    rule_vectors = embedder.embed_documents([item["rule"] for item in rules])
    chunk_vectors = embedder.embed_documents([item["excerpt"] for item in chunks])
    retrieved = retrieve_authority(
        rules, chunks, embedder, top_k=top_k,
        rule_vectors=rule_vectors, chunk_vectors=chunk_vectors,
    )
    near_pairs = near_rule_pairs(rules, embedder, vectors=rule_vectors)
    packets = []
    for offset in range(0, len(rules), batch_size):
        batch = rules[offset:offset + batch_size]
        batch_id = f"batch-{offset // batch_size + 1:03d}"
        packets.append({
            "batch_id": batch_id,
            "expected_names": [item["name"] for item in batch],
            "text": _packet(batch_id, batch, rules, retrieved, near_pairs),
        })
    return packets


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", type=Path, required=True)
    parser.add_argument("--authority-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--top-k", type=int, default=2)
    parser.add_argument("--batch-size", type=int, default=15)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--name", action="append", dest="names")
    args = parser.parse_args(argv)

    from compare_taste_outputs import ExactEmbeddingGemma

    packets = build_packets(
        args.results, args.authority_root, ExactEmbeddingGemma(), top_k=args.top_k,
        batch_size=args.batch_size, limit=args.limit,
        names=set(args.names) if args.names else None,
    )
    args.out_dir.mkdir(parents=True, exist_ok=True)
    manifest = []
    for packet in packets:
        path = args.out_dir / f"{packet['batch_id']}.md"
        path.write_text(packet["text"], encoding="utf-8")
        manifest.append({
            "batch_id": packet["batch_id"],
            "path": str(path),
            "chars": len(packet["text"]),
            "expected_names": packet["expected_names"],
        })
    (args.out_dir / "packet-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({
        "out_dir": str(args.out_dir), "packet_count": len(manifest),
        "candidate_count": sum(len(item["expected_names"]) for item in manifest),
        "packet_chars": [item["chars"] for item in manifest],
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
