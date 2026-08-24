#!/usr/bin/env python3
"""Conservatively consolidate semantically equivalent accepted Taste records."""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import re
from pathlib import Path

from adapt import adapt_llm, adapt_sessions, manifest


SYSTEM = """You deduplicate durable operator-preference records.

Input records already share scope, category, record type, and authority effect.
Return JSON only: {"groups": [["id", ...], ...]}.

Rules:
- Partition every input id exactly once.
- Group records only when they impose the same operational constraint.
- Wording differences alone may be grouped.
- Keep distinct rules separate when one is narrower, stronger, conditional, or adds a material requirement.
- Never merge tensions or contradictions.
- Use only exact ids from input. Do not rewrite rules.
"""

VERIFY_SYSTEM = """You independently verify proposed preference deduplication.

Return JSON only: {"equivalent": true|false, "reason": "<=20 words"}.
Return true only when every rule imposes the same operational constraint with no
materially different scope, strength, condition, threshold, mechanism, or extra requirement.
If uncertain, return false. Do not rewrite rules.
"""


class ConsolidationError(ValueError):
    pass


def _sha(value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _parse_partition(text: str, ids: set[str]) -> list[list[str]]:
    raw = text.strip()
    if raw.startswith("```"):
        raw = raw.split("\n", 1)[1].rsplit("```", 1)[0].strip()
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ConsolidationError(f"invalid JSON: {exc}") from exc
    groups = parsed.get("groups") if isinstance(parsed, dict) else None
    if not isinstance(groups, list) or not groups:
        raise ConsolidationError("response must contain nonempty groups")
    flattened: list[str] = []
    normalized: list[list[str]] = []
    for group in groups:
        if not isinstance(group, list) or not group or any(not isinstance(item, str) for item in group):
            raise ConsolidationError("each group must be a nonempty id list")
        normalized.append(sorted(group))
        flattened.extend(group)
    if len(flattened) != len(set(flattened)) or set(flattened) != ids:
        raise ConsolidationError("groups must partition exact input ids")
    return sorted(normalized, key=lambda group: group[0])


def _parse_verdict(text: str) -> tuple[bool, str]:
    raw = text.strip()
    if raw.startswith("```"):
        raw = raw.split("\n", 1)[1].rsplit("```", 1)[0].strip()
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ConsolidationError(f"invalid verification JSON: {exc}") from exc
    if not isinstance(parsed, dict) or not isinstance(parsed.get("equivalent"), bool):
        raise ConsolidationError("verification response needs boolean equivalent")
    return parsed["equivalent"], str(parsed.get("reason") or "")[:200]


def _dedupe(items: list, key) -> list:
    output = []
    seen = set()
    for item in items:
        marker = key(item)
        if marker in seen:
            continue
        seen.add(marker)
        output.append(item)
    return output


def _merge_group(records: list[dict]) -> dict:
    representative = min(records, key=lambda item: (len(item["rule"]), item["rule"], item["id"]))
    merged = json.loads(json.dumps(representative))
    merged["source_ids"] = sorted({value for item in records for value in item.get("source_ids", [])})
    merged["source_file_hashes"] = sorted(
        _dedupe(
            [value for item in records for value in item.get("source_file_hashes", [])],
            lambda value: (value.get("session_id"), value.get("sha256")),
        ),
        key=lambda value: (value.get("session_id", ""), value.get("sha256", "")),
    )
    merged["evidence_ids"] = sorted(
        _dedupe(
            [value for item in records for value in item.get("evidence_ids", [])],
            lambda value: (value.get("evidence_id"), value.get("source_session_id")),
        ),
        key=lambda value: (value.get("evidence_id", ""), value.get("source_session_id", "")),
    )
    merged["evidenceContexts"] = _dedupe(
        [value for item in records for value in item.get("evidenceContexts", [])],
        lambda value: _sha(value),
    )
    merged["retrieval_aliases"] = sorted({
        value for item in records for value in item.get("retrieval_aliases", []) if value
    })
    merged["evidence_count"] = len(merged["evidenceContexts"])
    merged["confidence"] = max(float(item.get("confidence", 0.0)) for item in records)
    if len(records) > 1:
        merged["human_note"] = (
            f"{merged.get('human_note', '')}; semantically consolidated from {len(records)} accepted records"
        ).strip("; ")
    merged["payload_sha256"] = manifest.payload_sha256(merged)
    return merged


def _bucket_key(record: dict) -> tuple[str, str, str, str]:
    return (
        record["scope"], record["category"], record["record_type"],
        record.get("authority_effect", "neutral"),
    )


def _has_material_extension(records: list[dict]) -> bool:
    token_sets = [set(re.findall(r"[a-z0-9]+", record["rule"].lower())) for record in records]
    return any(
        left < right and len(right - left) >= 4
        for left in token_sets for right in token_sets
    )


def _partition_bucket(records: list[dict], checkpoint_dir: Path, *, resume: bool) -> list[list[str]]:
    ordered = sorted(records, key=lambda item: item["id"])
    payload = [{"id": item["id"], "rule": item["rule"]} for item in ordered]
    digest = _sha(payload)
    checkpoint = checkpoint_dir / f"{digest}.json"
    if resume and checkpoint.is_file():
        saved = json.loads(checkpoint.read_text(encoding="utf-8"))
        return _parse_partition(json.dumps(saved["response"]), {item["id"] for item in ordered})
    user = json.dumps({"records": payload}, ensure_ascii=False)
    if not adapt_sessions.scan_batch_for_secrets_str(user):
        raise ConsolidationError("scanner-positive consolidation payload refused")
    response = adapt_llm._pi_response(SYSTEM, user, max_tokens=4096)
    groups = _parse_partition(response["text"], {item["id"] for item in ordered})
    checkpoint_dir.mkdir(parents=True, exist_ok=True)
    checkpoint.write_text(json.dumps({
        "input_sha256": digest,
        "response": {"groups": groups},
        "model": response.get("model"),
        "usage": response.get("usage") or {},
    }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return groups


def _verify_group(records: list[dict], checkpoint_dir: Path, *, resume: bool) -> tuple[bool, str]:
    ordered = sorted(records, key=lambda item: item["id"])
    payload = [{"id": item["id"], "rule": item["rule"]} for item in ordered]
    digest = _sha({"verification": payload})
    checkpoint = checkpoint_dir / f"verify-{digest}.json"
    if resume and checkpoint.is_file():
        saved = json.loads(checkpoint.read_text(encoding="utf-8"))
        return bool(saved["equivalent"]), str(saved.get("reason") or "")
    user = json.dumps({"proposed_group": payload}, ensure_ascii=False)
    if not adapt_sessions.scan_batch_for_secrets_str(user):
        raise ConsolidationError("scanner-positive verification payload refused")
    response = adapt_llm._pi_response(VERIFY_SYSTEM, user, max_tokens=1024)
    equivalent, reason = _parse_verdict(response["text"])
    checkpoint_dir.mkdir(parents=True, exist_ok=True)
    checkpoint.write_text(json.dumps({
        "input_sha256": digest,
        "equivalent": equivalent,
        "reason": reason,
        "model": response.get("model"),
        "usage": response.get("usage") or {},
    }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return equivalent, reason


def _verify_partitions(
    partitions: dict[tuple[str, str, str, str], list[list[str]]],
    records: dict[str, dict], checkpoint_dir: Path, *, workers: int, resume: bool,
) -> dict[tuple[str, str, str, str], list[list[str]]]:
    verified = {key: [group for group in groups if len(group) == 1] for key, groups in partitions.items()}
    work = [
        (key, group) for key, groups in partitions.items() for group in groups if len(group) > 1
    ]
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        futures = {
            pool.submit(
                _verify_group, [records[record_id] for record_id in group], checkpoint_dir,
                resume=resume,
            ): (key, group)
            for key, group in work
        }
        for future in concurrent.futures.as_completed(futures):
            key, group = futures[future]
            equivalent, _reason = future.result()
            if equivalent and _has_material_extension([records[record_id] for record_id in group]):
                equivalent = False
            verified[key].extend([group] if equivalent else [[record_id] for record_id in group])
    for groups in verified.values():
        groups.sort(key=lambda group: group[0])
    return verified


def consolidate(raw: dict, partitions: dict[tuple[str, str, str, str], list[list[str]]]) -> dict:
    output = json.loads(json.dumps(raw))
    accepted = [record for record in raw["records"] if record["status"] == "accepted"]
    by_id = {record["id"]: record for record in accepted}
    merged: list[dict] = []
    for key in sorted(partitions):
        for group in partitions[key]:
            merged.append(_merge_group([by_id[record_id] for record_id in group]))
    rejected = [record for record in raw["records"] if record["status"] != "accepted"]
    output["records"] = sorted(rejected + merged, key=lambda item: item["id"])
    output["generator"] = f"{raw.get('generator', 'adapt-manifest')}:semantic-equivalence-v1"
    return output


def run(input_path: Path, output_path: Path, checkpoint_dir: Path, *, workers: int, resume: bool) -> dict:
    raw = manifest.apply_time_validate(input_path)
    buckets: dict[tuple[str, str, str, str], list[dict]] = {}
    for record in raw["records"]:
        if record["status"] == "accepted":
            buckets.setdefault(_bucket_key(record), []).append(record)
    partitions: dict[tuple[str, str, str, str], list[list[str]]] = {}
    singletons = {key: [[items[0]["id"]]] for key, items in buckets.items() if len(items) == 1}
    partitions.update(singletons)
    multi = [(key, items) for key, items in buckets.items() if len(items) > 1]
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        futures = {
            pool.submit(_partition_bucket, items, checkpoint_dir, resume=resume): key
            for key, items in multi
        }
        for future in concurrent.futures.as_completed(futures):
            partitions[futures[future]] = future.result()
    accepted_by_id = {
        record["id"]: record for record in raw["records"] if record["status"] == "accepted"
    }
    partitions = _verify_partitions(
        partitions, accepted_by_id, checkpoint_dir, workers=workers, resume=resume,
    )
    result = consolidate(raw, partitions)
    output_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    manifest.apply_time_validate(output_path)
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--checkpoint-dir", type=Path, required=True)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--no-resume", action="store_true")
    args = parser.parse_args()
    before = manifest.apply_time_validate(args.input)
    result = run(
        args.input, args.output, args.checkpoint_dir,
        workers=args.workers, resume=not args.no_resume,
    )
    accepted_input = sum(item["status"] == "accepted" for item in before["records"])
    accepted_output = sum(item["status"] == "accepted" for item in result["records"])
    print(json.dumps({
        "contract": "same-scope-category-type-authority-semantic-equivalence-v1",
        "accepted_input": accepted_input,
        "accepted_output": accepted_output,
        "merged_records": accepted_input - accepted_output,
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
