#!/usr/bin/env python3
"""Run and strictly validate compact Morph conflict-review packets."""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Sequence


VERDICTS = {"safe", "flagged"}
KINDS = {
    "authority-conflict", "permission-expansion", "overbroad-scope", "duplicate",
    "contradiction", "transient", "unsupported",
}
WORKSPACE = Path(
    os.environ.get("WORKSPACE_ROOT") or Path(__file__).resolve().parents[5]
).expanduser().resolve()


def _json_object(raw: str) -> dict:
    text = re.sub(r"```(?:json)?", "", raw).strip()
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end <= start:
        raise ValueError("response does not contain a JSON object")
    value = json.loads(text[start:end + 1])
    if not isinstance(value, dict):
        raise ValueError("response is not a JSON object")
    return value


def validate_response(raw: str, *, batch_id: str, expected_names: Sequence[str]) -> dict:
    value = _json_object(raw)
    if value.get("batch_id") != batch_id:
        raise ValueError("wrong batch_id")
    reviews = value.get("reviews")
    if not isinstance(reviews, list):
        raise ValueError("reviews must be a list")
    parsed: dict[str, dict] = {}
    for review in reviews:
        if not isinstance(review, dict) or not isinstance(review.get("rule_name"), str):
            raise ValueError("invalid review row")
        name = review["rule_name"]
        if name in parsed:
            raise ValueError(f"duplicate review: {name}")
        verdict = review.get("verdict")
        findings = review.get("findings")
        if verdict not in VERDICTS or not isinstance(findings, list):
            raise ValueError(f"invalid verdict/findings: {name}")
        if verdict == "safe" and findings:
            raise ValueError(f"safe review has findings: {name}")
        if verdict == "flagged" and not findings:
            raise ValueError(f"flagged review has no findings: {name}")
        for finding in findings:
            if not isinstance(finding, dict) or finding.get("kind") not in KINDS:
                raise ValueError(f"invalid finding: {name}")
            if finding.get("severity") not in {"high", "medium", "low"}:
                raise ValueError(f"invalid severity: {name}")
            if not isinstance(finding.get("evidence"), str) or not finding["evidence"].strip():
                raise ValueError(f"finding lacks evidence: {name}")
        parsed[name] = {"verdict": verdict, "findings": findings}
    if set(parsed) != set(expected_names):
        raise ValueError("review names are missing or extra")
    if value.get("reviewed_rule_count") != len(expected_names):
        raise ValueError("reviewed_rule_count mismatch")
    return {"batch_id": batch_id, "reviews": parsed, "reviewed_rule_count": len(parsed)}


def validate_worker_results(worker_results: Sequence[dict], packet_manifest: Sequence[dict]) -> dict:
    expected = {item["batch_id"]: item["expected_names"] for item in packet_manifest}
    validated = []
    for item in worker_results:
        item_id = str(item.get("id", ""))
        batch_match = re.search(r"(batch-\d{3})$", item_id)
        row = {"id": item_id, "ok": False, "raw": item}
        if not batch_match or batch_match.group(1) not in expected:
            row["error"] = "worker id does not identify a known batch"
        elif not item.get("ok"):
            row["error"] = str(item.get("error", "provider failure"))
        else:
            batch_id = batch_match.group(1)
            try:
                row["validated"] = validate_response(
                    str(item.get("output", "")),
                    batch_id=batch_id,
                    expected_names=expected[batch_id],
                )
                row["ok"] = True
            except (ValueError, json.JSONDecodeError) as exc:
                row["error"] = f"{type(exc).__name__}: {exc}"
        validated.append(row)
    return {
        "jobs": validated,
        "job_count": len(validated),
        "valid_count": sum(item["ok"] for item in validated),
        "invalid_count": sum(not item["ok"] for item in validated),
    }


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    os.replace(temporary, path)


def load_worker_results(path: Path) -> list[dict]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(value, dict) and isinstance(value.get("worker_stdout"), str):
        value = json.loads(value["worker_stdout"])
    if not isinstance(value, list):
        raise ValueError("worker results must be a JSON list")
    return value


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--api-manifest", type=Path)
    source.add_argument("--worker-results", type=Path)
    parser.add_argument("--packet-manifest", type=Path, required=True)
    parser.add_argument(
        "--worker",
        type=Path,
        default=WORKSPACE / "tools" / "skills" / "coder" / "scripts" / "api-worker.py",
    )
    parser.add_argument("--pool-size", type=int, default=4)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    packet_manifest = json.loads(args.packet_manifest.read_text(encoding="utf-8"))
    if args.worker_results:
        worker_results = load_worker_results(args.worker_results)
        result = validate_worker_results(worker_results, packet_manifest)
        result["source_worker_results"] = str(args.worker_results)
    else:
        completed = subprocess.run(
            [sys.executable, str(args.worker), "--batch", str(args.api_manifest),
             "--pool-size", str(args.pool_size)],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
        )
        try:
            worker_results = json.loads(completed.stdout)
        except json.JSONDecodeError:
            result = {
                "job_count": 0, "valid_count": 0, "invalid_count": 1,
                "worker_error": completed.stderr.strip() or f"exit {completed.returncode}",
                "worker_stdout": completed.stdout,
            }
        else:
            result = validate_worker_results(worker_results, packet_manifest)
            result["worker_returncode"] = completed.returncode
            result["worker_stderr"] = completed.stderr.strip()
    _write_json(args.out, result)
    print(json.dumps({key: result.get(key) for key in (
        "job_count", "valid_count", "invalid_count", "worker_error"
    ) if key in result}, indent=2))
    return 0 if result.get("invalid_count") == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
