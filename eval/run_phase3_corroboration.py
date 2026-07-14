#!/usr/bin/env python3
"""Blind semantic corroboration panel for frozen Phase 2 Adapt candidates."""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import importlib.util
import json
import os
import re
import sys
import threading
import time
from pathlib import Path
from typing import Callable, Sequence


WS = Path("D:/Claude")
ADAPT_DIR = WS / "tools/pipelines/memory/adapt"
JURY_DIR = WS / "tools/review"
SYSTEM = """You are a conservative adjudicator for coding-agent preference memory.
Return only a JSON array with exactly one object per input item.

ADMIT only when the evidence explicitly supports a durable, cross-task instruction about how the
coding agent should behave and the proposed rule does not broaden it. REJECT task-specific requests,
product/business decisions, episodic facts, pasted documentation or quoted policy, model-generated
meta text, and permission-expanding rules. QUARANTINE genuine ambiguity.

Judge semantic sufficiency, not evidence frequency. One explicit excerpt can establish a durable
instruction. Do not reject or quarantine merely because source_session_count is 1. Direct present-
tense rules using "from now on", "always", "never", "I prefer", or an explicit correction can be
durable when they govern agent workflow, verification, tooling, style, documentation, architecture,
model routing, memory handling, or how the agent communicates its work. Direct user instructions
about the agent itself are valid; `pasted_or_meta` applies only to copied or model-authored text.
By contrast, a temporary gate containing "until", a current stack declaration, a product/UI
decision, or a request to perform the current task is not an agent preference.

Examples that qualify: "always use JSONL for structured logs", "never expire tokens that are still
referenced", "from now on mark sessions learned only after every batch succeeds", and "I prefer
Fable for architecture". Examples that do not: "do not start this backfill until resume is wired",
"the active stack is Rust and Tauri", and "make this checkout page match Rotten Hand".

Each output object must be:
{"id":"...","verdict":"admit|quarantine|reject","flags":[],"reason":"brief evidence-based reason"}
Allowed flags: task_specific, product_or_fact, pasted_or_meta, overgeneralized,
permission_expanding, authority_conflict, ambiguous. Never infer durability from words such as
always or never alone. Every item must be judged by the same standard; origin and labels are hidden."""

VERDICTS = {"admit", "quarantine", "reject"}
FLAGS = {
    "task_specific", "product_or_fact", "pasted_or_meta", "overgeneralized",
    "permission_expanding", "authority_conflict", "ambiguous",
}
HARD_FLAGS = {"permission_expanding", "authority_conflict"}
MODEL_SPECS = {
    "minimax-m3-direct": ("minimax", "MiniMax-M3"),
    "deepseek-v4-pro-nim": ("nim", "deepseek-ai/deepseek-v4-pro"),
    "kimi-k2.6-nim": ("nim", "moonshotai/kimi-k2.6"),
    "nemotron-super-nim": ("nim", "nvidia/nemotron-3-super-120b-a12b"),
    "qwen3.6-27b-groq": ("groq", "qwen/qwen3.6-27b"),
    "gpt-oss-120b-groq": ("groq", "openai/gpt-oss-120b"),
}


def _sha(data: object) -> str:
    raw = json.dumps(data, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def build_cases(
    results: dict, observations: dict, labels: Sequence[dict]
) -> tuple[list[dict], dict[str, str], dict[str, bool]]:
    by_observation = {
        item["observation_id"]: item for item in observations.get("observations", [])
    }
    cases: list[dict] = []
    ownership: dict[str, str] = {}
    for arm, arm_result in results.get("arms", {}).items():
        for candidate in arm_result.get("candidates", []):
            candidate_id = candidate["id"]
            if candidate_id in ownership:
                raise ValueError(f"duplicate candidate id: {candidate_id}")
            support = [
                by_observation[item_id]
                for item_id in candidate.get("supporting_observation_ids", [])
                if item_id in by_observation
            ]
            evidence = list(dict.fromkeys(
                item["evidence"] for item in support if item.get("evidence")
            ))
            sessions = {
                item["source_session_id"] for item in support
                if item.get("source_session_id")
            }
            cases.append({
                "id": candidate_id,
                "kind": "candidate",
                "rule": candidate.get("rule", ""),
                "category": candidate.get("category", ""),
                "evidence": evidence,
                "source_session_count": len(sessions),
            })
            ownership[candidate_id] = arm

    expected: dict[str, bool] = {}
    for label in labels:
        control_id = f"control-{label['id']}"
        excerpt = label["excerpt"]
        cases.append({
            "id": control_id,
            "kind": "control",
            "rule": excerpt,
            "category": "unknown",
            "evidence": [excerpt],
            "source_session_count": 1,
        })
        expected[control_id] = bool(
            label.get("is_agent_preference", label.get("is_preference", False))
        )
    cases.sort(key=lambda item: hashlib.sha256(item["id"].encode()).hexdigest())
    return cases, ownership, expected


def parse_verdicts(raw: str, expected_ids: set[str]) -> dict[str, dict]:
    text = re.sub(r"```(?:json)?", "", raw)
    allowed = "|".join(sorted((re.escape(flag) for flag in FLAGS), key=len, reverse=True))
    text = re.sub(
        rf"(?P<prefix>[\[,]\s*)(?P<flag>{allowed})(?=\s*[,\]])",
        lambda match: f'{match.group("prefix")}"{match.group("flag")}"',
        text,
    )
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    try:
        decoded = json.loads(text.strip())
    except json.JSONDecodeError:
        decoded = None
    if (isinstance(decoded, dict) and set(decoded) == {"items"}
            and isinstance(decoded["items"], list)):
        data = decoded["items"]
    elif lines and all(line.startswith("{") and line.endswith("}") for line in lines):
        # MiniMax sometimes honors the object schema but emits strict JSONL.
        # Exact IDs below still refuse omissions or extra decisions.
        data = [json.loads(line) for line in lines]
    else:
        start, end = text.find("["), text.rfind("]")
        if start < 0 or end <= start:
            raise ValueError("response does not contain a JSON array or JSONL objects")
        data = json.loads(text[start:end + 1])
    if not isinstance(data, list):
        raise ValueError("response is not a JSON array")
    parsed: dict[str, dict] = {}
    for item in data:
        if not isinstance(item, dict) or not isinstance(item.get("id"), str):
            raise ValueError("invalid verdict item")
        item_id = item["id"]
        verdict = item.get("verdict")
        if verdict not in VERDICTS:
            raise ValueError(f"unknown verdict: {verdict}")
        flags = item.get("flags", [])
        if not isinstance(flags, list) or any(flag not in FLAGS for flag in flags):
            raise ValueError(f"unknown flags for {item_id}")
        reason = item.get("reason", "")
        if not isinstance(reason, str):
            raise ValueError(f"invalid reason for {item_id}")
        normalized = {"verdict": verdict, "flags": sorted(set(flags)), "reason": reason}
        if item_id in parsed:
            prior = parsed[item_id]
            if (prior["verdict"], prior["flags"]) != (normalized["verdict"], normalized["flags"]):
                raise ValueError(f"conflicting duplicate verdict id: {item_id}")
            continue
        parsed[item_id] = normalized
    if set(parsed) != expected_ids:
        raise ValueError("response ids are missing or extra")
    return parsed


def _calibrate(verdicts: dict[str, dict], expected: dict[str, bool]) -> dict:
    tp = fp = fn = tn = 0
    for item_id, wanted in expected.items():
        emitted = verdicts[item_id]["verdict"] == "admit"
        if wanted and emitted:
            tp += 1
        elif wanted:
            fn += 1
        elif emitted:
            fp += 1
        else:
            tn += 1
    precision = tp / max(1, tp + fp)
    recall = tp / max(1, tp + fn)
    return {
        "tp": tp, "fp": fp, "fn": fn, "tn": tn,
        "precision": round(precision, 3), "recall": round(recall, 3),
        "eligible": precision >= 0.90 and recall >= 0.60,
    }


def aggregate_votes(
    *,
    votes: dict[str, dict[str, dict]],
    expected_controls: dict[str, bool],
    candidate_ids: set[str],
    ownership: dict[str, str],
) -> dict:
    models = {name: _calibrate(items, expected_controls) for name, items in votes.items()}
    eligible = [name for name, metrics in models.items() if metrics["eligible"]]
    candidates: dict[str, dict] = {}
    for candidate_id in sorted(candidate_ids):
        item_votes = {name: votes[name][candidate_id] for name in eligible}
        hard_flags = sorted({
            flag for item in item_votes.values() for flag in item["flags"] if flag in HARD_FLAGS
        })
        admit_count = sum(item["verdict"] == "admit" for item in item_votes.values())
        reject_count = sum(item["verdict"] == "reject" for item in item_votes.values())
        if hard_flags:
            status, reason = "quarantined", "hard safety flag"
        elif admit_count >= 2 and admit_count > reject_count:
            status, reason = "admitted", "two calibrated corroborators"
        elif reject_count >= 2:
            status, reason = "rejected", "two calibrated rejections"
        else:
            status, reason = "quarantined", "insufficient calibrated consensus"
        candidates[candidate_id] = {
            "arm": ownership[candidate_id],
            "status": status,
            "reason": reason,
            "hard_flags": hard_flags,
            "votes": item_votes,
        }
    return {
        "models": models,
        "eligible_models": eligible,
        "candidates": candidates,
        "counts_by_arm": _counts_by_arm(candidates),
    }


def _counts_by_arm(candidates: dict[str, dict]) -> dict[str, dict[str, int]]:
    result: dict[str, dict[str, int]] = {}
    for item in candidates.values():
        counts = result.setdefault(item["arm"], {status: 0 for status in (
            "admitted", "quarantined", "rejected"
        )})
        counts[item["status"]] += 1
    return result


def _write_json_atomic(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(data, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def run_panel(
    *,
    cases: Sequence[dict],
    model_ids: Sequence[str],
    output_dir: Path,
    call_fn: Callable[[str, str, str, int], str],
    scanner: Callable[[str], bool],
    chunk_size: int,
    max_provider_calls: int,
    max_tokens: int = 4096,
    max_workers: int = 5,
    resume: bool,
    inter_call_delay_s: float = 0.0,
) -> dict:
    if chunk_size < 1:
        raise ValueError("chunk_size must be positive")
    if not 1 <= max_workers <= 5:
        raise ValueError("max_workers must be in [1, 5]")
    chunks = [list(cases[index:index + chunk_size]) for index in range(0, len(cases), chunk_size)]
    ledger = {"provider_calls": 0, "cache_hits": 0, "input_chars": 0}
    votes: dict[str, dict[str, dict]] = {}
    failures: dict[str, str] = {}
    calls_dir = output_dir / "calls"
    calls_dir.mkdir(parents=True, exist_ok=True)
    pending_jobs: list[dict] = []
    expected_counts = {model_id: len(cases) for model_id in model_ids}

    for model_id in model_ids:
        model_votes: dict[str, dict] = {}
        model_jobs: list[dict] = []
        safe_model_id = re.sub(r"[^a-zA-Z0-9_.-]+", "-", model_id)
        try:
            for chunk_index, chunk in enumerate(chunks, 1):
                original_by_opaque: dict[str, str] = {}
                blinded = []
                for item_index, item in enumerate(chunk, 1):
                    opaque_id = f"item-{item_index:03d}"
                    original_by_opaque[opaque_id] = item["id"]
                    visible = {
                        key: value for key, value in item.items()
                        if key not in {"id", "kind"}
                    }
                    visible["id"] = opaque_id
                    blinded.append(visible)
                user = json.dumps({"items": blinded}, ensure_ascii=False, separators=(",", ":"))
                request = {
                    "model_id": model_id,
                    "system": SYSTEM,
                    "user": user,
                    "max_tokens": max_tokens,
                }
                request_sha256 = _sha(request)
                call_path = calls_dir / f"{safe_model_id}-{chunk_index:03d}.json"
                expected_ids = set(original_by_opaque)
                if resume and call_path.exists():
                    cached = json.loads(call_path.read_text(encoding="utf-8"))
                    if cached.get("request_sha256") != request_sha256:
                        raise ValueError(f"cached request mismatch: {call_path.name}")
                    try:
                        cached_verdicts = parse_verdicts(cached["response"], expected_ids)
                    except Exception:
                        if cached.get("status") == "success":
                            raise
                    else:
                        model_votes.update({
                            original_by_opaque[item_id]: verdict
                            for item_id, verdict in cached_verdicts.items()
                        })
                        ledger["cache_hits"] += 1
                        continue
                elif call_path.exists():
                    raise FileExistsError(f"call cache exists; use --resume: {call_path}")
                if not scanner(SYSTEM + "\n" + user):
                    raise RuntimeError(f"secret scanner blocked {model_id} chunk {chunk_index}")
                model_jobs.append({
                    "model_id": model_id, "chunk_index": chunk_index,
                    "user": user, "request_sha256": request_sha256,
                    "call_path": call_path, "expected_ids": expected_ids,
                    "original_by_opaque": original_by_opaque,
                })
        except Exception as exc:
            failures[model_id] = f"{type(exc).__name__}: {exc}"
            continue
        votes[model_id] = model_votes
        pending_jobs.extend(model_jobs)

    if len(pending_jobs) > max_provider_calls:
        raise RuntimeError("provider-call ceiling exceeded")

    def execute(job: dict) -> tuple[str, dict[str, dict]]:
        started = time.time()
        response = call_fn(job["model_id"], SYSTEM, job["user"], max_tokens)
        envelope = {
            "model_id": job["model_id"],
            "chunk_index": job["chunk_index"],
            "request_sha256": job["request_sha256"],
            "response": response,
            "response_sha256": hashlib.sha256(response.encode("utf-8")).hexdigest(),
            "elapsed_s": round(time.time() - started, 3),
            "status": "success",
        }
        try:
            parsed = parse_verdicts(response, job["expected_ids"])
        except Exception as exc:
            envelope["status"] = "invalid_response"
            envelope["error"] = f"{type(exc).__name__}: {exc}"
            _write_json_atomic(job["call_path"], envelope)
            raise
        _write_json_atomic(job["call_path"], envelope)
        remapped = {
            job["original_by_opaque"][item_id]: verdict
            for item_id, verdict in parsed.items()
        }
        return job["model_id"], remapped

    ledger["provider_calls"] = len(pending_jobs)
    ledger["input_chars"] = sum(len(SYSTEM) + len(job["user"]) for job in pending_jobs)
    if pending_jobs:
        with ThreadPoolExecutor(max_workers=min(max_workers, len(pending_jobs))) as executor:
            futures = {}
            last_submit: float | None = None
            for job in pending_jobs:
                if last_submit is not None and inter_call_delay_s > 0:
                    remaining = inter_call_delay_s - (time.monotonic() - last_submit)
                    if remaining > 0:
                        time.sleep(remaining)
                futures[executor.submit(execute, job)] = job
                last_submit = time.monotonic()
            for future in as_completed(futures):
                job = futures[future]
                try:
                    model_id, remapped = future.result()
                except Exception as exc:
                    failures[job["model_id"]] = f"{type(exc).__name__}: {exc}"
                else:
                    votes.setdefault(model_id, {}).update(remapped)

    for model_id in list(votes):
        if model_id in failures or len(votes[model_id]) != expected_counts[model_id]:
            votes.pop(model_id, None)
    return {"votes": votes, "failures": failures, "ledger": ledger}


_PROVIDERS: dict[str, object] = {}
_PROVIDERS_LOCK = threading.Lock()


def _call_model(model_id: str, system: str, user: str, max_tokens: int) -> str:
    provider_name, model = MODEL_SPECS[model_id]
    if provider_name == "minimax":
        sys.path.insert(0, str(ADAPT_DIR))
        import adapt_llm
        response = adapt_llm.call_lane_response(
            system, user, lane="minimax", max_tokens=max_tokens,
            attempts=1, thinking="disabled", temperature=0.0,
        )
        return response["text"]

    if provider_name not in _PROVIDERS:
        with _PROVIDERS_LOCK:
            if provider_name not in _PROVIDERS:
                sys.path.insert(0, str(JURY_DIR))
                import yaml
                from providers import build_provider
                config = yaml.safe_load((JURY_DIR / "models.yaml").read_text(encoding="utf-8"))
                provider_config = dict(config["providers"][provider_name])
                if provider_name == "groq":
                    # Keep this local so ordinary Qwen jury calls retain reasoning mode.
                    extra = dict(provider_config.get("model_extra_body") or {})
                    extra["qwen/qwen3.6-27b"] = {"reasoning_effort": "none"}
                    provider_config["model_extra_body"] = extra
                _PROVIDERS[provider_name] = build_provider(provider_name, provider_config)
    return _PROVIDERS[provider_name].call(model, system, user, max_tokens=max_tokens)


def _load_jsonl(path: Path) -> list[dict]:
    return [
        json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def _planned_input_chars(cases: Sequence[dict], model_count: int, chunk_size: int) -> int:
    chunks = [cases[index:index + chunk_size] for index in range(0, len(cases), chunk_size)]
    one_model = sum(
        len(SYSTEM) + len(json.dumps({"items": chunk}, ensure_ascii=False, separators=(",", ":")))
        for chunk in chunks
    )
    return one_model * model_count


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase2-dir", type=Path, required=True)
    parser.add_argument("--labels", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--models", nargs="+", choices=sorted(MODEL_SPECS),
                        default=list(MODEL_SPECS))
    parser.add_argument("--chunk-size", type=int, default=20)
    parser.add_argument("--max-provider-calls", type=int, default=12)
    parser.add_argument("--max-input-chars", type=int, default=250_000)
    parser.add_argument("--max-tokens", type=int, default=4096)
    parser.add_argument("--workers", type=int, default=5)
    parser.add_argument("--inter-call-delay", type=float, default=0.0)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)

    results_path = args.phase2_dir / "results.json"
    observations_path = args.phase2_dir / "observations.json"
    results = json.loads(results_path.read_text(encoding="utf-8"))
    observations = json.loads(observations_path.read_text(encoding="utf-8"))
    labels = _load_jsonl(args.labels)
    cases, ownership, expected = build_cases(results, observations, labels)
    planned_calls = len(args.models) * ((len(cases) + args.chunk_size - 1) // args.chunk_size)
    planned_chars = _planned_input_chars(cases, len(args.models), args.chunk_size)
    preflight = {
        "candidate_count": len(ownership),
        "control_count": len(expected),
        "case_count": len(cases),
        "models": args.models,
        "chunk_size": args.chunk_size,
        "planned_provider_calls": planned_calls,
        "planned_input_chars": planned_chars,
        "max_provider_calls": args.max_provider_calls,
        "max_input_chars": args.max_input_chars,
        "within_ceilings": (
            planned_calls <= args.max_provider_calls and planned_chars <= args.max_input_chars
        ),
        "live_memright_writes": 0,
        "live_state_writes": 0,
    }
    if args.dry_run:
        print(json.dumps(preflight, ensure_ascii=False, indent=2))
        return 0 if preflight["within_ceilings"] else 2
    if not preflight["within_ceilings"]:
        parser.error("preflight exceeds provider-call or input-character ceiling")
    if not 1 <= args.workers <= 5:
        parser.error("--workers must be in [1, 5]")
    if args.out.exists() and not args.resume:
        parser.error(f"output exists; use --resume: {args.out}")
    args.out.mkdir(parents=True, exist_ok=True)
    _write_json_atomic(args.out / "cases.json", {
        "schema_version": "1.0.0", "cases": cases,
        "cases_sha256": _sha(cases),
    })
    _write_json_atomic(args.out / "preflight.json", preflight)

    sys.path.insert(0, str(ADAPT_DIR))
    import adapt_sessions
    panel = run_panel(
        cases=cases,
        model_ids=args.models,
        output_dir=args.out,
        call_fn=_call_model,
        scanner=adapt_sessions.scan_batch_for_secrets_str,
        chunk_size=args.chunk_size,
        max_provider_calls=args.max_provider_calls,
        max_tokens=args.max_tokens,
        max_workers=args.workers,
        resume=args.resume,
        inter_call_delay_s=args.inter_call_delay,
    )
    aggregate = aggregate_votes(
        votes=panel["votes"], expected_controls=expected,
        candidate_ids=set(ownership), ownership=ownership,
    )
    report = {
        "schema_version": "1.0.0",
        "phase2_results_sha256": hashlib.sha256(results_path.read_bytes()).hexdigest(),
        "observations_sha256": hashlib.sha256(observations_path.read_bytes()).hexdigest(),
        "labels_sha256": hashlib.sha256(args.labels.read_bytes()).hexdigest(),
        "cases_sha256": _sha(cases),
        "preflight": preflight,
        "ledger": panel["ledger"],
        "provider_failures": panel["failures"],
        **aggregate,
        "side_effects": {"memright_calls": 0, "live_state_writes": 0},
    }
    report["content_sha256"] = _sha(report)
    _write_json_atomic(args.out / "results.json", report)
    summary = {
        "eligible_models": report["eligible_models"],
        "provider_failures": report["provider_failures"],
        "counts_by_arm": report["counts_by_arm"],
        "ledger": report["ledger"],
        "results": str(args.out / "results.json"),
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0 if len(report["eligible_models"]) >= 2 else 1


if __name__ == "__main__":
    raise SystemExit(main())
