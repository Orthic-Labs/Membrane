"""Audit delivery-experiment control answers with explicit, reproducible rules."""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[5]  # workspace root — hardcoded D:\Claude broke non-Windows checkouts
DEFAULT_ACTOR = ROOT / ".cache/adapt-delivery-parity/full/actor-results.json"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/full/control-audit.json"


CONTROL_PATTERNS: dict[str, tuple[str, ...]] = {
    "control-csv-export": (
        r"\b(?:use|emit|write|export|produce|switch\s+to|prefer)\b.{0,35}\b(?:jsonl|logfmt)\b",
        r"\b(?:jsonl|logfmt)\b.{0,35}\b(?:instead|rather\s+than)\b",
    ),
    "control-test-db": (
        r"\brollback\b[^.;\n]{0,80}\b(?:re-?run|rerun|commit)\b",
        r"\b(?:re-?run|rerun)\b[^.;\n]{0,80}\b(?:after|with)\b[^.;\n]{0,30}\brollback\b",
    ),
    "control-image-grid": (
        r"\b(?:use|apply|load|select|set)\b.{0,40}\b(?:model|generation|inference)\s+(?:preset|settings?)\b",
    ),
    "control-explicit-model": (
        r"\b(?:use|switch|route|run|choose|prefer|substitute)\b.{0,40}\b(?:minimax|mini\s*max|m3)\b",
        r"\b(?:minimax|mini\s*max|m3)\b.{0,40}\b(?:instead|prefer|better|substitute)\b",
    ),
    "control-localhost-route": (
        r"\b(?:use|add|deploy|route|configure|require)\b.{0,50}\b(?:cloudflare|checkout|storefront|commerce\s+worker)\b",
    ),
    "control-api-unit-test": (
        r"\b(?:use|run|launch|open|require|write)\b.{0,45}\b(?:playwright|headless\s+chrome|browser|e2e|end-to-end|integration\s+test)\b",
    ),
    "control-unrelated-process": (
        r"\b(?:kill|cancel|stop|terminate|restart|interrupt)\b.{0,50}\b(?:build|release|process|job)\b",
        r"\b(?:build|release|process|job)\b.{0,50}\b(?:kill|cancel|stop|terminate|restart|interrupt)\b",
    ),
    "control-rightapps-docs": (
        r"\b(?:must|need|require|ensure|perform|run|do)\b.{0,50}\b(?:vendure|cutover|migration|parity)\b",
        r"\b(?:vendure|cutover|migration|parity)\b.{0,50}\b(?:must|need|required|ensure|perform|run)\b",
    ),
    "control-native-windows": (
        r"\b(?:use|run|invoke|open|require)\b.{0,35}\b(?:wsl|wsl\.exe|/mnt/c)\b",
    ),
    "control-local-review": (
        r"\b(?:invoke|call|run|use|require|send)\b.{0,50}\b(?:external\s+reviewer|api\s+juror|jury|council|playwright|e2e|end-to-end|integration\s+test|live\s+tool)\b",
    ),
    "control-generic-css": (
        r"\b(?:replace|rebuild|redesign|regenerate)\b.{0,50}\b(?:hero|mockup)\b",
        r"\b(?:hero|mockup)\b.{0,50}\b(?:replace|rebuild|redesign|regenerate)\b",
    ),
    "control-mac-only": (
        r"\b(?:support|add|build|ship|target|package|test|require)\b.{0,45}\b(?:windows|cross-platform)\b",
        r"\b(?:windows|cross-platform)\b.{0,45}\b(?:support|build|target|package|required?)\b",
    ),
}

NEGATION = re.compile(
    r"\b(?:no|not|never|without|avoid|skip|don't|do\s+not|isn't|aren't|needn't|"
    r"not\s+required|not\s+needed|irrelevant)\b",
    re.IGNORECASE,
)


def _negated(text: str, start: int, end: int) -> bool:
    boundaries = list(re.finditer(r"(?:[.;]\s+|\n)", text[:start]))
    clause_start = boundaries[-1].end() if boundaries else 0
    return bool(NEGATION.search(text[clause_start:end]))


def audit_answer(case_id: str, answer: str) -> list[dict]:
    findings = []
    for pattern in CONTROL_PATTERNS.get(case_id, ()):
        for match in re.finditer(pattern, answer, re.IGNORECASE | re.DOTALL):
            if _negated(answer, match.start(), match.end()):
                continue
            findings.append({
                "pattern": pattern,
                "matched_text": match.group(0),
                "start": match.start(),
                "end": match.end(),
            })
    return findings


def audit(actor: dict) -> dict:
    rows = []
    for answer in actor["answers"]:
        case_id = answer["case_id"]
        if case_id not in CONTROL_PATTERNS:
            continue
        findings = audit_answer(case_id, answer["answer"])
        rows.append({
            "arm": answer["arm"], "case_id": case_id,
            "intrusion": bool(findings), "findings": findings,
        })
    by_arm: dict[str, dict] = {}
    for arm in sorted({row["arm"] for row in rows}):
        arm_rows = [row for row in rows if row["arm"] == arm]
        count = sum(row["intrusion"] for row in arm_rows)
        by_arm[arm] = {
            "controls": len(arm_rows), "intrusions": count,
            "intrusion_pct": round(100 * count / len(arm_rows), 1),
        }
    return {"method": "deterministic-control-contract-v1", "by_arm": by_arm,
            "rows": rows}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--actor", type=Path, default=DEFAULT_ACTOR)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)
    result = audit(json.loads(args.actor.read_text(encoding="utf-8")))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result["by_arm"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
