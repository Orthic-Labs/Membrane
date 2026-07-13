"""Build/validate the Adapt value-set (paired prompts for the Gate 0 A/B).

The value-set is the operator-labeled dataset the Gate 0 evaluator grades against.
Each row must include a stable `id`, the raw prompt, an arm (preference_relevant
or unrelated_control), and the expected applicable rule ids.

This script:
  1. Loads value_set_schema.json
  2. Validates every row against schema
  3. Verifies minItems >= 100 (per Gate 0)
  4. Verifies that arm proportions look like a real labeler (some relevance,
    some controls) — does NOT enforce a hard ratio
  5. Resolves each `expected_applicable_rule_ids` entry against the current
    Adapt pool so rules named before deletion are caught at build time
  6. Emits a frozen COPY to docs/baselines/adapt/value_set_<date>.jsonl on
    successful validation, so the harness grades against an immutable record

Run from workspace root:
  py -3.11 tools/pipelines/memory/adapt/eval/build_value_set.py <value_set.json>
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path

WS = Path("D:/Claude")
ADAPT_DIR = WS / "tools" / "pipelines" / "memory" / "adapt"
SCHEMA_PATH = ADAPT_DIR / "eval" / "value_set_schema.json"
BASELINE_DIR = WS / "docs" / "baselines" / "adapt"


def _load_schema() -> dict:
    return json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))


def _fetch_current_rule_ids() -> set[str]:
    """Read live MemRight rows of the form D--Claude/adapt-<slug>."""
    try:
        out = subprocess.check_output(
            [str(WS / "tools" / "bin" / "memright.exe"), "list"],
            text=True, timeout=15,
        )
    except Exception as exc:
        sys.stderr.write(f"warning: could not list memright ({exc}); "
                         f"skipping rule-id existence checks\n")
        return set()
    return {line.split()[-1].split("/")[-1] for line in out.splitlines()
            if "/adapt-" in line}


def _hash_prompt(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()[:12]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("value_set", type=Path,
                    help="path to the value-set JSON file to validate")
    ap.add_argument("--no-freeze", action="store_true",
                    help="skip writing the immutable baseline copy (test runs only)")
    args = ap.parse_args()

    if not args.value_set.exists():
        sys.stderr.write(f"error: {args.value_set} not found\n")
        return 2

    schema = _load_schema()
    # Lazy jsonschema import; tolerate absence.
    try:
        import jsonschema  # type: ignore
    except ImportError:
        sys.stderr.write("error: jsonschema not installed; "
                         f"pip install jsonschema\n")
        return 2

    raw = json.loads(args.value_set.read_text(encoding="utf-8"))
    jsonschema.validate(raw, schema)

    prompts = raw["prompts"]
    counter = Counter(p["arm"] for p in prompts)
    relevant = counter.get("preference_relevant", 0)
    control = counter.get("unrelated_control", 0)
    if len(prompts) < 100:
        sys.stderr.write(f"error: Gate 0 requires ≥100 prompts, got {len(prompts)}\n")
        return 2
    if relevant == 0 or control == 0:
        sys.stderr.write("error: value-set must contain BOTH arms\n")
        return 2
    if relevant < 50 or control < 50:
        sys.stderr.write(f"warning: arm imbalance (relevant={relevant}, "
                         f"control={control}); plan target is ≥50 each\n")

    # Validate expected-rule-id existence against current MemRight pool.
    current_ids = _fetch_current_rule_ids()
    dead_refs: list[tuple[str, str]] = []
    for p in prompts:
        for rid in p.get("expected_applicable_rule_ids", []):
            if current_ids and rid not in current_ids:
                dead_refs.append((p["id"], rid))
        for rid in p.get("expected_contraindicated_rule_ids", []):
            if current_ids and rid not in current_ids:
                dead_refs.append((p["id"], rid))
    if dead_refs:
        sys.stderr.write(f"warning: {len(dead_refs)} prompt(s) reference "
                         f"rule ids not in current MemRight pool:\n")
        for pid, rid in dead_refs[:20]:
            sys.stderr.write(f"  - prompt {pid} -> {rid}\n")

    # Cross-check duplicate prompts (each prompt id is unique within the set)
    seen: dict[str, str] = {}
    dup_ids: list[str] = []
    for p in prompts:
        if p["id"] in seen:
            dup_ids.append(p["id"])
        seen[p["id"]] = p.get("prompt", "")
    if dup_ids:
        sys.stderr.write(f"error: duplicate prompt ids: {dup_ids[:5]}\n")
        return 2

    # Optional frozen baseline copy
    if not args.no_freeze:
        BASELINE_DIR.mkdir(parents=True, exist_ok=True)
        date = dt.date.today().isoformat()
        out = BASELINE_DIR / f"value_set_{date}.jsonl"
        with out.open("w", encoding="utf-8") as fh:
            for p in prompts:
                fh.write(json.dumps(p, ensure_ascii=False) + "\n")
        meta = {
            "source": str(args.value_set),
            "frozen_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "prompt_count": len(prompts),
            "schema_version": raw.get("schema_version", "1.0.0"),
            "reviewer": raw.get("reviewer", ""),
            "prompt_hashes": {p["id"]: _hash_prompt(p.get("prompt", ""))
                               for p in prompts[:50]},
        }
        (BASELINE_DIR / f"value_set_{date}.meta.json").write_text(
            json.dumps(meta, indent=2, ensure_ascii=False), encoding="utf-8")
        print(f"frozen baseline: {out}")

    print(f"validation passed: {len(prompts)} prompts "
          f"(relevant={relevant}, control={control})")
    if dead_refs:
        print(f"  (warning) {len(dead_refs)} dead rule-id refs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
