#!/usr/bin/env python3
"""Resolve a pending Adapt manifest through blinded calibrated consensus."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence


ROOT = Path(__file__).resolve().parent
DEFAULT_CONTROLS = ROOT / "tests" / "labeled_corpus.jsonl"
DEFAULT_MODELS = ("minimax-m3-direct",)
DEFAULT_PRIMARY_MODEL = "minimax-m3-direct"


def _load_phase3():
    path = ROOT / "eval" / "run_phase3_corroboration.py"
    spec = importlib.util.spec_from_file_location("adapt_phase3", path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot load corroboration panel: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_manifest_module():
    path = ROOT / "manifest.py"
    spec = importlib.util.spec_from_file_location("adapt_manifest_contract", path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot load manifest contract: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_jsonl(path: Path) -> list[dict]:
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def _sha(value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def build_cases(manifest: dict, controls: Sequence[dict]) -> tuple[list[dict], dict[str, bool]]:
    cases: list[dict] = []
    for record in manifest["records"]:
        if record["status"] != "pending":
            continue
        excerpts = [
            item.get("excerpt", "")
            for item in record.get("evidence_ids", [])
            if item.get("excerpt")
        ]
        if not excerpts and record.get("evidence_excerpt"):
            excerpts = [record["evidence_excerpt"]]
        cases.append({
            "id": record["id"],
            "kind": "candidate",
            "rule": record["rule"],
            "category": record["category"],
            "evidence": list(dict.fromkeys(excerpts)),
            "source_session_count": len(set(record.get("source_ids") or [])),
        })

    expected: dict[str, bool] = {}
    for control in controls:
        control_id = f"control-{control['id']}"
        excerpt = control["excerpt"]
        cases.append({
            "id": control_id,
            "kind": "control",
            "rule": excerpt,
            "category": control.get("category") or "unknown",
            "evidence": [excerpt],
            "source_session_count": 1,
        })
        expected[control_id] = bool(
            control.get("is_agent_preference", control.get("is_preference", False))
        )
    cases.sort(key=lambda item: hashlib.sha256(item["id"].encode()).hexdigest())
    return cases, expected


def resolve_manifest(manifest: dict, panel: dict) -> tuple[dict, dict]:
    resolved = json.loads(json.dumps(manifest))
    decisions = []
    for record in resolved["records"]:
        if record["status"] != "pending":
            decisions.append({
                "id": record["id"], "status": record["status"],
                "needs_review": bool(record.get("needs_review", False)),
                "reason": "preserved deterministic decision",
            })
            continue
        decision = panel.get("candidates", {}).get(record["id"])
        admitted = bool(decision and decision.get("status") == "admitted")
        record["status"] = "accepted" if admitted else "rejected"
        record["needs_review"] = not admitted
        reason = decision.get("reason") if decision else "missing panel decision"
        record["human_note"] = f"automated adjudication: {reason}"
        decisions.append({
            "id": record["id"], "status": record["status"],
            "needs_review": record["needs_review"], "reason": reason,
            "hard_flags": decision.get("hard_flags", []) if decision else [],
            "votes": decision.get("votes", {}) if decision else {},
        })

    resolved["generator"] = "adapt-adjudicate-manifest/1"
    counts = {
        "accepted": sum(item["status"] == "accepted" for item in decisions),
        "rejected": sum(item["status"] == "rejected" for item in decisions),
        "needs_review": sum(item["needs_review"] for item in decisions),
    }
    audit = {
        "schema_version": "1.0.0",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "input_batch_id": manifest["batch_id"],
        "eligible_models": panel.get("eligible_models", []),
        "models": panel.get("models", {}),
        "provider_failures": panel.get("provider_failures", {}),
        "decision_policy": panel.get("decision_policy", "unknown"),
        "primary_model": panel.get("primary_model"),
        "counts": counts,
        "decisions": decisions,
    }
    audit["content_sha256"] = _sha(audit)
    return resolved, audit


def aggregate_primary(*, panel_module, votes: dict, expected_controls: dict,
                      candidate_ids: set[str], primary_model: str) -> dict:
    models = {
        name: panel_module._calibrate(items, expected_controls)
        for name, items in votes.items()
    }
    eligible = [name for name, metrics in models.items() if metrics["eligible"]]
    primary_votes = votes.get(primary_model, {}) if primary_model in eligible else {}
    candidates = {}
    for candidate_id in sorted(candidate_ids):
        vote = primary_votes.get(candidate_id)
        hard_flags = sorted(set(vote.get("flags", [])) & panel_module.HARD_FLAGS) if vote else []
        if not vote:
            status, reason = "quarantined", "primary model unavailable or uncalibrated"
        elif hard_flags:
            status, reason = "quarantined", "hard safety flag"
        elif vote["verdict"] == "admit":
            status, reason = "admitted", "calibrated primary admit"
        elif vote["verdict"] == "reject":
            status, reason = "rejected", "calibrated primary reject"
        else:
            status, reason = "quarantined", "calibrated primary quarantine"
        candidates[candidate_id] = {
            "arm": "production-manifest",
            "status": status,
            "reason": reason,
            "hard_flags": hard_flags,
            "votes": {primary_model: vote} if vote else {},
        }
    return {
        "models": models,
        "eligible_models": eligible,
        "primary_model": primary_model,
        "candidates": candidates,
        "counts_by_arm": panel_module._counts_by_arm(candidates),
    }


def _merge_ledgers(*ledgers: dict) -> dict:
    return {
        key: sum(int(ledger.get(key, 0)) for ledger in ledgers)
        for key in ("provider_calls", "cache_hits", "input_chars")
    }


def _has_call_cache(path: Path) -> bool:
    calls = path / "calls"
    return calls.is_dir() and any(calls.glob("*.json"))


def run_calibrated_panel(
    *, panel_module, cases: Sequence[dict], expected_controls: dict[str, bool],
    model_ids: Sequence[str], calibration_dir: Path, calls_dir: Path,
    call_fn, scanner, chunk_size: int, max_provider_calls: int,
    max_tokens: int, max_workers: int, resume: bool,
    inter_call_delay_s: float,
) -> dict:
    control_cases = [item for item in cases if item.get("kind") == "control"]
    candidate_cases = [item for item in cases if item.get("kind") == "candidate"]
    calibration = panel_module.run_panel(
        cases=control_cases, model_ids=model_ids, output_dir=calibration_dir,
        call_fn=call_fn, scanner=scanner, chunk_size=chunk_size,
        max_provider_calls=max_provider_calls, max_tokens=max_tokens,
        max_workers=max_workers, resume=_has_call_cache(calibration_dir),
        inter_call_delay_s=inter_call_delay_s,
    )
    calibration_models = {
        name: panel_module._calibrate(items, expected_controls)
        for name, items in calibration["votes"].items()
    }
    eligible = [
        name for name in model_ids
        if calibration_models.get(name, {}).get("eligible")
    ]
    remaining_calls = max_provider_calls - calibration["ledger"]["provider_calls"]
    candidate = {
        "votes": {}, "failures": {},
        "ledger": {"provider_calls": 0, "cache_hits": 0, "input_chars": 0},
    }
    if eligible and candidate_cases:
        candidate = panel_module.run_panel(
            cases=candidate_cases, model_ids=eligible, output_dir=calls_dir,
            call_fn=call_fn, scanner=scanner, chunk_size=chunk_size,
            max_provider_calls=remaining_calls, max_tokens=max_tokens,
            max_workers=max_workers, resume=resume,
            inter_call_delay_s=inter_call_delay_s,
        )
    combined_votes = {
        model_id: dict(items) for model_id, items in calibration["votes"].items()
    }
    for model_id, items in candidate["votes"].items():
        combined_votes.setdefault(model_id, {}).update(items)
    failures = dict(calibration["failures"])
    failures.update(candidate["failures"])
    return {
        "votes": combined_votes,
        "failures": failures,
        "ledger": _merge_ledgers(calibration["ledger"], candidate["ledger"]),
        "calibration_models": calibration_models,
    }


def _write_json_atomic(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def write_outputs(output_path: Path, audit_path: Path, resolved: dict, audit: dict) -> None:
    _write_json_atomic(output_path, resolved)
    _write_json_atomic(audit_path, audit)


def validate_distinct_paths(manifest_path: Path, output_path: Path, audit_path: Path) -> None:
    resolved = {path.resolve() for path in (manifest_path, output_path, audit_path)}
    if len(resolved) != 3:
        raise ValueError("--manifest, --out, and --audit must be distinct paths")


def _planned_input_chars(panel, cases: Sequence[dict], model_count: int, chunk_size: int) -> int:
    return panel._planned_input_chars(cases, model_count, chunk_size)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--audit", type=Path, required=True)
    parser.add_argument("--calls-dir", type=Path, required=True)
    parser.add_argument("--calibration-dir", type=Path)
    parser.add_argument("--controls", type=Path, default=DEFAULT_CONTROLS)
    parser.add_argument("--models", nargs="+", default=list(DEFAULT_MODELS))
    parser.add_argument("--decision-policy", choices=("calibrated-primary", "dual-consensus"),
                        default="calibrated-primary")
    parser.add_argument("--primary-model", default=DEFAULT_PRIMARY_MODEL)
    parser.add_argument("--chunk-size", type=int, default=5)
    parser.add_argument("--max-provider-calls", type=int, default=20)
    parser.add_argument("--max-input-chars", type=int, default=250_000)
    parser.add_argument("--max-tokens", type=int, default=4096)
    parser.add_argument("--workers", type=int, default=5)
    parser.add_argument("--inter-call-delay", type=float, default=0.0)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)
    try:
        validate_distinct_paths(args.manifest, args.out, args.audit)
    except ValueError as exc:
        parser.error(str(exc))

    panel = _load_phase3()
    calibration_dir = args.calibration_dir or (args.calls_dir / "calibration")
    unknown = sorted(set(args.models) - set(panel.MODEL_SPECS))
    if unknown:
        parser.error(f"unknown models: {unknown}")
    if (args.decision_policy == "calibrated-primary"
            and args.primary_model not in args.models):
        parser.error("--primary-model must be included in --models")
    contract = _load_manifest_module()
    pending = contract.validate_schema(args.manifest)
    controls = _load_jsonl(args.controls)
    cases, expected = build_cases(pending, controls)
    candidate_ids = {
        record["id"] for record in pending["records"] if record["status"] == "pending"
    }
    planned_calls = len(args.models) * ((len(cases) + args.chunk_size - 1) // args.chunk_size)
    planned_chars = _planned_input_chars(panel, cases, len(args.models), args.chunk_size)
    preflight = {
        "pending_candidates": len(candidate_ids),
        "controls": len(expected),
        "models": args.models,
        "planned_provider_calls": planned_calls,
        "planned_input_chars": planned_chars,
        "within_ceilings": (
            planned_calls <= args.max_provider_calls
            and planned_chars <= args.max_input_chars
        ),
        "live_crypt_writes": 0,
        "live_state_writes": 0,
    }
    if args.dry_run:
        print(json.dumps(preflight, ensure_ascii=False, indent=2))
        return 0 if preflight["within_ceilings"] else 2
    if not candidate_ids:
        parser.error("manifest has no pending candidates")
    if not 1 <= args.workers <= 5:
        parser.error("--workers must be in [1, 5]")
    if not preflight["within_ceilings"]:
        parser.error("preflight exceeds provider-call or input-character ceiling")
    for path in (args.out, args.audit):
        if path.exists() and not args.resume:
            parser.error(f"output exists; use --resume: {path}")

    sys.path.insert(0, str(ROOT))
    import adapt_sessions
    run = run_calibrated_panel(
        panel_module=panel, cases=cases, expected_controls=expected,
        model_ids=args.models, calibration_dir=calibration_dir,
        calls_dir=args.calls_dir / "candidates",
        call_fn=panel._call_model,
        scanner=adapt_sessions.scan_batch_for_secrets_str,
        chunk_size=args.chunk_size,
        max_provider_calls=args.max_provider_calls,
        max_tokens=args.max_tokens,
        max_workers=args.workers,
        resume=args.resume,
        inter_call_delay_s=args.inter_call_delay,
    )
    if args.decision_policy == "calibrated-primary":
        aggregate = aggregate_primary(
            panel_module=panel, votes=run["votes"], expected_controls=expected,
            candidate_ids=candidate_ids, primary_model=args.primary_model,
        )
    else:
        aggregate = panel.aggregate_votes(
            votes=run["votes"],
            expected_controls=expected,
            candidate_ids=candidate_ids,
            ownership={record_id: "production-manifest" for record_id in candidate_ids},
        )
    aggregate["decision_policy"] = args.decision_policy
    aggregate["provider_failures"] = run["failures"]
    resolved, audit = resolve_manifest(pending, aggregate)
    audit["input_manifest_sha256"] = hashlib.sha256(args.manifest.read_bytes()).hexdigest()
    audit["controls_sha256"] = hashlib.sha256(args.controls.read_bytes()).hexdigest()
    audit["cases_sha256"] = _sha(cases)
    audit["ledger"] = run["ledger"]
    audit["content_sha256"] = _sha({
        key: value for key, value in audit.items() if key != "content_sha256"
    })
    write_outputs(args.out, args.audit, resolved, audit)
    contract.apply_time_validate(args.out)
    print(json.dumps({
        "eligible_models": audit["eligible_models"],
        "provider_failures": audit["provider_failures"],
        "counts": audit["counts"],
        "ledger": audit["ledger"],
        "resolved_manifest": str(args.out),
        "audit": str(args.audit),
    }, ensure_ascii=False, indent=2))
    if args.decision_policy == "calibrated-primary":
        return 0 if args.primary_model in audit["eligible_models"] else 1
    return 0 if len(audit["eligible_models"]) >= 2 else 1


if __name__ == "__main__":
    raise SystemExit(main())
