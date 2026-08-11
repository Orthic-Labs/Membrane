"""Compile root-scoped standing preferences into a bounded always-loaded core."""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
from pathlib import Path


ROOT = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
HERE = Path(__file__).resolve().parent
ADAPT_DIR = HERE.parent
DEFAULT_TREATMENT = ROOT / ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/compiled-core/core.json"

SYSTEM = """Compile durable preferences into a compact always-loaded core.
Input contains root-scoped standing preferences only. Deduplicate semantic overlaps. Keep only
broad defaults that are useful across unrelated engineering tasks and safe to inject globally.
Drop product-, model-, tool-, format-, database-, email-, ASR-, UI-element-, and one-off workflow
specific rules; those remain retrievable. Do not invent policy. Each output rule must cite one or
more input source_ids. Return ONLY JSON:
{"rules":[{"text":"imperative, <=18 words","source_ids":["..."]}]}
Use 6-14 rules. Total rendered policy must stay under 2,800 characters."""


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


runner = _load(HERE / "run_delivery_parity.py", "budgeted_core_runner")


def validate(payload: dict, source_ids: set[str]) -> tuple[str, int]:
    rules = payload.get("rules")
    if not isinstance(rules, list) or not 6 <= len(rules) <= 14:
        raise ValueError("compiled core must contain 6-14 rules")
    for row in rules:
        text = row.get("text")
        ids = row.get("source_ids")
        if not isinstance(text, str) or not text.strip() or len(text.split()) > 18:
            raise ValueError("each core rule must be non-empty and <=18 words")
        if not isinstance(ids, list) or not ids or not set(ids) <= source_ids:
            raise ValueError("each core rule needs valid source_ids")
    block = "\n".join(f"[core] {row['text'].strip()}" for row in rules)
    tokens = math.ceil(len(block) / 4)
    if len(block) > 2800 or tokens > 800:
        raise ValueError(f"compiled core exceeds budget: chars={len(block)} tokens={tokens}")
    return block, tokens


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--treatment", type=Path, default=DEFAULT_TREATMENT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args(argv)
    treatment = runner.read_json(args.treatment)
    sources = [
        {"source_id": row["id"], "rule": row["rule"]}
        for row in treatment["records"]
        if row["record_type"] == "standing_preference" and row["scope"] == "D--Claude"
    ]
    if len(sources) != 21:
        raise RuntimeError(f"expected 21 root standing preferences, found {len(sources)}")
    request = json.dumps({"preferences": sources}, ensure_ascii=False)
    if not runner.adapt_sessions.scan_batch_for_secrets_str(request):
        raise RuntimeError("secret scanner blocked core-digest payload")
    input_hash = runner.sha_json({"system": SYSTEM, "preferences": sources})
    if args.resume and args.out.exists():
        cached = runner.read_json(args.out)
        if cached.get("input_sha256") == input_hash:
            print(json.dumps(cached, indent=2))
            return 0
    response = runner.adapt_llm.call_lane_response(
        SYSTEM, request, lane="minimax", max_tokens=3000, attempts=2,
        thinking="adaptive", temperature=0.1)
    cleaned = response["text"].strip()
    if cleaned.startswith("```"):
        cleaned = cleaned.split("\n", 1)[1].rsplit("```", 1)[0].strip()
    parsed = json.loads(cleaned)
    payload = parsed if isinstance(parsed, dict) else {"rules": parsed}
    block, tokens = validate(payload, {row["source_id"] for row in sources})
    result = {
        "input_sha256": input_hash, "source_count": len(sources),
        "rules": payload["rules"], "policy_block": block,
        "chars": len(block), "estimated_tokens": tokens,
        "provider": {k: response.get(k) for k in
                     ("model", "stop_reason", "stop_sequence", "usage")},
        "raw_response": response["text"],
    }
    runner.write_json(args.out, result)
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
