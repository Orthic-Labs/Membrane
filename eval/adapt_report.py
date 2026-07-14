"""Adapt report — audit + journal + state dashboard (v2 plan acceptance criteria).

The v2 plan's Acceptance Criteria require:
> "the Adapt report also tracks rejection reasons, conflicts, stale rules,
>  rollback events, token cost, and latency. Review these after the 10-session
>  apply and each later chunk; no new monitoring service is required for this
>  local on-demand phase."

This script is the local on-demand report. It reads only the machine-local
state under ``~/.claude/adapt/`` (audit.jsonl, run_journal.jsonl, state.json,
rules.json, safepoints/) and emits both a structured JSON report and a
markdown summary. No service dependency.

Outputs::

    <out>/adapt.report.json    structured counts + recent events
    <out>/adapt.report.md      human-readable summary

Every Adapt run writes audit entries with the same shape; the report extracts
rejection reasons, admission decisions, memright-write failures, manifest
applications, and rollback events. Latency is the wall-clock difference
between ``discovered`` and ``committed`` journal entries per batch.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import sqlite3
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

WS = Path(__file__).resolve().parents[5]  # workspace root — hardcoded D:/Claude broke non-Windows checkouts
ADAPT_STATE = Path.home() / ".claude" / "adapt"
AUDIT = ADAPT_STATE / "audit.jsonl"
JOURNAL = ADAPT_STATE / "run_journal.jsonl"
STATE_FILE = ADAPT_STATE / "state.json"
RULES_FILE = ADAPT_STATE / "rules.json"
SAFEPOINTS = ADAPT_STATE / "safepoints"
HEARTBEAT = WS / "tools/.cache/metrics/memright-heartbeat.jsonl"
MEMRIGHT_DB = WS / "tools/.cache/memory/memright-engine.db"


def _load_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    out: list[dict] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def _safe_load(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def _batch_latency_seconds(journal_entries: list[dict]) -> dict[str, float]:
    """Wall-clock per batch_id from discovered -> committed."""
    by_batch: dict[str, dict[str, str]] = defaultdict(dict)
    for e in journal_entries:
        bid = e.get("batch_id")
        stage = e.get("stage")
        ts = e.get("ts")
        if not bid or not stage or not ts:
            continue
        # Stage ordering matters: discovered always comes first.
        if stage not in by_batch[bid]:
            by_batch[bid][stage] = ts
    latencies: dict[str, float] = {}
    for bid, stages in by_batch.items():
        if "discovered" in stages and "committed" in stages:
            try:
                t0 = dt.datetime.fromisoformat(stages["discovered"])
                t1 = dt.datetime.fromisoformat(stages["committed"])
                latencies[bid] = (t1 - t0).total_seconds()
            except ValueError:
                continue
    return latencies


def _effectiveness(db_path: Path | None) -> dict:
    injected: set[str] = set()
    fetched: set[str] = set()
    if db_path and db_path.exists():
        try:
            conn = sqlite3.connect(db_path)
            try:
                rows = conn.execute(
                    "SELECT event_kind, memory_id FROM memory_event_log "
                    "WHERE memory_id LIKE '%/adapt-%' OR memory_id LIKE 'adapt-%'"
                )
                for event, memory_id in rows:
                    if event == "inject":
                        injected.add(memory_id)
                    elif event == "get":
                        fetched.add(memory_id)
            finally:
                conn.close()
        except sqlite3.Error:
            pass
    explicitly_used = fetched & injected
    return {
        "injected_unique": len(injected),
        "explicit_get_unique": len(explicitly_used),
        "explicit_usefulness_rate": (
            round(len(explicitly_used) / len(injected), 3) if injected else 0.0
        ),
    }


def build_report(state_dir: Path = ADAPT_STATE,
                 heartbeat_path: Path = HEARTBEAT,
                 db_path: Path | None = MEMRIGHT_DB) -> dict:
    audit = _load_jsonl(state_dir / "audit.jsonl")
    journal = _load_jsonl(state_dir / "run_journal.jsonl")
    state = _safe_load(state_dir / "state.json")
    rules = _safe_load(state_dir / "rules.json")
    heartbeat = _load_jsonl(heartbeat_path)

    # Audit-derived counts.
    rejection_reasons: Counter[str] = Counter()
    actions: Counter[str] = Counter()
    write_failures: list[dict] = []
    manifest_events: list[dict] = []
    rollback_events: list[dict] = []
    for entry in audit:
        ev = entry.get("event")
        action = entry.get("action")
        if action:
            actions[action] += 1
        if ev == "admission_rejected":
            rejection_reasons[entry.get("why", "unknown")] += 1
        elif ev == "memright_write_failed":
            write_failures.append(entry)
        elif ev in ("manifest_applied", "manifest_apply_failed",
                    "manifest_rollback_deleted"):
            manifest_events.append(entry)
        elif ev == "rollback":
            rollback_events.append(entry)
        rejection_reasons[str(entry).startswith("{") and ""]  # no-op safeguard
    # LLM call failures (from adapt_llm).
    llm_failures = [e for e in audit if e.get("event") == "llm_call_failed"]

    receipts = [e for e in heartbeat if e.get("event") == "recall.delivery"]
    applicable_ids = {
        memory_id for receipt in receipts
        for memory_id in receipt.get("applicable_adapt_ids", [])
    }
    delivered_ids = {
        memory_id for receipt in receipts
        for memory_id in receipt.get("delivered_adapt_ids", [])
    }
    shadow = {
        "delivery_receipts": len(receipts),
        "applicable_occurrences": sum(
            len(e.get("applicable_adapt_ids", [])) for e in receipts
        ),
        "applicable_unique": len(applicable_ids),
        "delivered_occurrences": sum(
            len(e.get("delivered_adapt_ids", [])) for e in receipts
        ),
        "delivered_unique": len(delivered_ids),
        "core_deliveries": sum(bool(e.get("core_delivered")) for e in receipts),
        "context_chars": sum(int(e.get("context_chars", 0)) for e in receipts),
        "estimated_tokens": sum(int(e.get("estimated_tokens", 0)) for e in receipts),
        "core_tokens": sum(int(e.get("core_tokens", 0)) for e in receipts),
        "retrieval_tokens": sum(int(e.get("retrieval_tokens", 0)) for e in receipts),
        "conflicts": sum(e.get("event") == "shadow_conflict" for e in audit),
        "recorrections": sum(e.get("event") == "shadow_recorrection" for e in audit),
        **_effectiveness(db_path),
    }

    # Journal-derived counts.
    batches = {e["batch_id"] for e in journal if "batch_id" in e}
    completed = {e["batch_id"] for e in journal
                 if e.get("stage") == "committed"}
    incomplete = sorted(batches - completed)
    latencies = _batch_latency_seconds(journal)

    # State-derived.
    learned_total = 0
    for tool, sessions in (state.get("learned") or {}).items():
        learned_total += len(sessions)
    rules_count = len(rules)

    # Safepoints.
    safepoint_count = 0
    if state_dir.joinpath("safepoints").exists():
        safepoint_count = sum(1 for p in state_dir.joinpath("safepoints").glob("*.json"))

    return {
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "rules_count": rules_count,
        "learned_sessions_total": learned_total,
        "audit_total": len(audit),
        "batch_total": len(batches),
        "batches_completed": len(completed),
        "batches_incomplete": incomplete,
        "actions": dict(actions),
        "admission_rejection_reasons": dict(rejection_reasons),
        "memright_write_failures": len(write_failures),
        "llm_call_failures": len(llm_failures),
        "manifest_events": len(manifest_events),
        "rollback_events": len(rollback_events),
        "safepoints": safepoint_count,
        "shadow": shadow,
        "latency_per_batch_seconds": latencies,
        "latency_avg_seconds": statistics.mean(latencies.values()) if latencies else 0.0,
        "latency_max_seconds": max(latencies.values()) if latencies else 0.0,
    }


def _format_report(rep: dict) -> str:
    L = ["# Adapt report", ""]
    L.append(f"- generated: {rep['generated_at']}")
    L.append(f"- rules in rules.json: {rep['rules_count']}")
    L.append(f"- learned sessions total: {rep['learned_sessions_total']}")
    L.append(f"- audit entries: {rep['audit_total']}")
    L.append("")
    L.append("## Batches")
    L.append("")
    L.append(f"- total batches: {rep['batch_total']}")
    L.append(f"- completed: {rep['batches_completed']}")
    L.append(f"- incomplete: {len(rep['batches_incomplete'])}")
    if rep["batches_incomplete"]:
        L.append("  - " + ", ".join(rep["batches_incomplete"][:10]))
    L.append("")
    L.append("## Actions")
    L.append("")
    for k, v in sorted(rep["actions"].items()):
        L.append(f"- {k}: {v}")
    L.append("")
    L.append("## Rejections")
    L.append("")
    if rep["admission_rejection_reasons"]:
        for k, v in sorted(rep["admission_rejection_reasons"].items()):
            L.append(f"- {k}: {v}")
    else:
        L.append("- (none)")
    L.append("")
    L.append("## Failure signals")
    L.append("")
    L.append(f"- memright write failures: {rep['memright_write_failures']}")
    L.append(f"- llm call failures:        {rep['llm_call_failures']}")
    L.append(f"- manifest events:         {rep['manifest_events']}")
    L.append(f"- rollback events:         {rep['rollback_events']}")
    L.append("")
    L.append("## Latency")
    L.append("")
    L.append(f"- avg wall per batch: {rep['latency_avg_seconds']:.1f}s")
    L.append(f"- max wall per batch: {rep['latency_max_seconds']:.1f}s")
    L.append(f"- safepoints on disk: {rep['safepoints']}")
    L.append("")
    L.append("## Shadow delivery")
    L.append("")
    shadow = rep["shadow"]
    L.append(f"- delivery receipts: {shadow['delivery_receipts']}")
    L.append(f"- applicable unique IDs: {shadow['applicable_unique']}")
    L.append(f"- delivered unique IDs: {shadow['delivered_unique']}")
    L.append(f"- core deliveries: {shadow['core_deliveries']}")
    L.append(f"- estimated tokens: {shadow['estimated_tokens']}")
    L.append(f"- core / retrieval tokens: {shadow['core_tokens']} / "
             f"{shadow['retrieval_tokens']}")
    L.append(f"- explicit get/usefulness proxy: {shadow['explicit_get_unique']}/"
             f"{shadow['injected_unique']} ({shadow['explicit_usefulness_rate']:.3f})")
    L.append(f"- conflicts: {shadow['conflicts']}")
    L.append(f"- re-corrections: {shadow['recorrections']}")
    return "\n".join(L) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--state-dir", type=Path, default=ADAPT_STATE)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--heartbeat", type=Path, default=HEARTBEAT)
    ap.add_argument("--db", type=Path, default=MEMRIGHT_DB)
    args = ap.parse_args(argv)

    rep = build_report(
        state_dir=args.state_dir, heartbeat_path=args.heartbeat, db_path=args.db
    )
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "adapt.report.json").write_text(
        json.dumps(rep, indent=2, ensure_ascii=False), encoding="utf-8")
    (args.out / "adapt.report.md").write_text(_format_report(rep),
                                              encoding="utf-8")
    print(f"report: {args.out / 'adapt.report.md'}")
    print(f"  rules: {rep['rules_count']}, "
          f"learned_sessions: {rep['learned_sessions_total']}, "
          f"batches: {rep['batch_total']}/{rep['batches_completed']} completed, "
          f"admission rejections: "
          f"{sum(rep['admission_rejection_reasons'].values())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
