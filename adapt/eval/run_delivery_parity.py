"""Run the five-arm CommandCode Taste versus Adapt delivery experiment.

Arms:
  A  current Cortex retrieval only
  T  A + the real 52-rule CommandCode Taste file always loaded
  B  copied Cortex + all 57 raw usable Adapt records, retrieved normally
  C  A + the complete 57-record Adapt digest always loaded
  D  B + the complete 57-record Adapt digest always loaded

Only copied SQLite databases are modified. Provider stages are content-hash
cached so ``--resume`` never repeats a completed actor or grader batch.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import random
import sqlite3
import subprocess
import sys
import time
from pathlib import Path


ROOT = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
HERE = Path(__file__).resolve().parent
ADAPT_DIR = HERE.parent
PACKAGE_DIR = ADAPT_DIR / "src" / "adapt"
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/full"
DEFAULT_LIVE_DB = ROOT / "tools/.cache/memory/cortex-engine.db"
DEFAULT_MEMBRANE = ROOT / "tools/bin/membrane"
API_WORKER = ROOT / "legion/skills/coder/scripts/api-worker.py"
DEFAULT_GRADER_MODEL = "deepseek-ai/deepseek-v4-pro"
TOP_K = 5
ACTOR_CONTEXT_K = 3
ACTOR_BATCH_SIZE = 30
GRADER_BATCH_SIZE = 30
ARMS = ("A", "T", "B", "C", "D")


def _load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if not spec or not spec.loader:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


builder = _load_module(HERE / "build_delivery_parity_set.py", "delivery_parity_builder")
value_ab = _load_module(HERE / "run_value_ab.py", "delivery_parity_value_ab")
adapt_llm = _load_module(PACKAGE_DIR / "adapt_llm.py", "delivery_parity_adapt_llm")
adapt_sessions = _load_module(PACKAGE_DIR / "adapt_sessions.py", "delivery_parity_sessions")
control_audit = _load_module(HERE / "audit_delivery_controls.py", "delivery_control_audit")

_run_via_port = value_ab._run_via_port


def sha_json(value: object) -> str:
    raw = json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(value, indent=2, ensure_ascii=False), encoding="utf-8")
    tmp.replace(path)


def read_json(path: Path) -> dict | list:
    return json.loads(path.read_text(encoding="utf-8"))


def select_cases(cases: list[dict], smoke: bool) -> list[dict]:
    if not smoke:
        return list(cases)
    selected = []
    for cohort in ("shared", "taste_only", "adapt_only"):
        selected.append(next(row for row in cases if row["cohort"] == cohort))
    selected.extend(row for row in cases if row["cohort"] == "control")
    return selected[:5]


def memory_rows(db: Path) -> dict[str, dict]:
    with sqlite3.connect(db) as conn:
        cols = {row[1] for row in conn.execute("PRAGMA table_info(memories)")}
        if "scope_id" in cols:
            rows = conn.execute("SELECT id, content, scope_id FROM memories")
            return {mid: {"text": text or "", "scope": scope or ""}
                    for mid, text, scope in rows}
        rows = conn.execute("SELECT id, content FROM memories")
        return {mid: {"text": text or "", "scope": ""} for mid, text in rows}


def replay_all(cortex: Path, db: Path, port: int, cases: list[dict],
               input_path: Path) -> dict[str, dict]:
    """One replay subprocess for the whole arm, followed by direct DB lookup."""
    input_path.parent.mkdir(parents=True, exist_ok=True)
    input_path.write_text("".join(json.dumps({
        "row_id": row["case_id"], "query": row["prompt"], "scope": row["scope"],
    }, ensure_ascii=False) + "\n" for row in cases), encoding="utf-8")
    started = time.monotonic()
    output = _run_via_port(
        cortex, db, port, ["replay", "--input", str(input_path), "-k", str(TOP_K)])
    wall_ms = int((time.monotonic() - started) * 1000)
    parsed = [json.loads(line) for line in output.splitlines() if line.strip()]
    if {row["row_id"] for row in parsed} != {row["case_id"] for row in cases}:
        raise RuntimeError("replay output case coverage mismatch")
    contents = memory_rows(db)
    result = {}
    for row in parsed:
        ids = row.get("ranked_ids", [])
        records = [contents.get(mid, {"text": "", "scope": ""}) for mid in ids]
        result[row["row_id"]] = {
            "ranked_ids": ids,
            "ranked_texts": [record["text"] for record in records],
            "ranked_scopes": [record["scope"] for record in records],
            "wall_ms_batch": wall_ms,
        }
    return result


def _run_replay_db(cortex: Path, live_db: Path, db: Path, cases: list[dict],
                   input_path: Path, records: list[dict] | None = None) -> tuple[dict, dict]:
    before = value_ab.snapshot_live_db(live_db, db)
    port = value_ab._free_port()
    service = value_ab._start_service(cortex, db, port)
    try:
        value_ab._wait_ready(port)
        for record in records or []:
            value_ab.put_pref_record(
                cortex, db, port, record["id"], record.get("scope", "D--Claude"),
                value_ab.adapt_record_envelope(record),
            )
        ranked = replay_all(cortex, db, port, cases, input_path)
    finally:
        value_ab._stop_service(service)
    ok, count, msg = value_ab.integrity_check(db)
    if not ok:
        raise RuntimeError(f"copied DB integrity failed: {msg}")
    return ranked, {"base_rows": before, "final_rows": count, "integrity": msg}


def run_retrieval(value_set: dict, treatment: dict, cases: list[dict], out: Path,
                  live_db: Path, cortex: Path, resume: bool) -> dict:
    path = out / "retrieval.json"
    input_hash = sha_json({"value": value_set["content_sha256"],
                           "cases": [row["case_id"] for row in cases],
                           "adapt_records": treatment["records_sha256"]})
    if resume and path.exists():
        cached = read_json(path)
        if cached.get("input_sha256") == input_hash:
            return cached
    pre_ok, pre_count, pre_msg = value_ab.integrity_check(live_db)
    if not pre_ok:
        raise RuntimeError(f"live DB integrity failed before experiment: {pre_msg}")
    ranked_a, integrity_a = _run_replay_db(
        cortex, live_db, out / "A.db", cases, out / "inputs/A.jsonl")
    ranked_b, integrity_b = _run_replay_db(
        cortex, live_db, out / "B.db", cases, out / "inputs/B.jsonl",
        treatment["records"])
    post_ok, post_count, post_msg = value_ab.integrity_check(live_db)
    if not post_ok or post_count != pre_count:
        raise RuntimeError("live Cortex DB changed during isolated experiment")
    result = {
        "input_sha256": input_hash,
        "live_db": {"path": str(live_db), "rows_before": pre_count,
                    "rows_after": post_count, "integrity": post_msg,
                    "unchanged": post_count == pre_count},
        "copies": {"A": integrity_a, "B": integrity_b},
        "A": ranked_a, "B": ranked_b,
    }
    write_json(path, result)
    return result


def taste_block(value_set: dict) -> str:
    return "\n".join(
        f"[taste/{row['category']}] {row['rule']}"
        for row in value_set["taste_rules"]
    )


def adapt_block(treatment: dict) -> str:
    return "\n".join(
        f"[adapt/{row['record_type']}/{row['category']}/{row['scope']}] {row['rule']}"
        for row in treatment["records"]
    )


def select_adapt_treatment(source: dict, variant: str) -> dict:
    selected = dict(source)
    if variant == "raw":
        records = list(source["records"])
    elif variant == "curated":
        records = list(source["curated_records"])
    else:
        raise ValueError(f"unknown Adapt treatment variant: {variant}")
    selected["variant"] = variant
    selected["records"] = records
    selected["records_sha256"] = sha_json(records)
    selected["selected_record_count"] = len(records)
    return selected


def _safe_context(retrieved: dict) -> list[str]:
    contexts = []
    for text, scope in zip(retrieved.get("ranked_texts", []),
                           retrieved.get("ranked_scopes", [])):
        scope_l = scope.casefold()
        if "health" in scope_l or "medical" in scope_l:
            continue
        cleaned = adapt_sessions.redact(text).strip()
        if cleaned:
            contexts.append(cleaned[:1800])
        if len(contexts) == ACTOR_CONTEXT_K:
            break
    return contexts


def arm_case_payload(arm: str, case: dict, retrieval: dict,
                     source_key: str | None = None) -> dict:
    source_name = source_key or ("B" if arm in {"B", "D"} else "A")
    source = retrieval[source_name]
    contexts = _safe_context(source[case["case_id"]])
    return {"case_id": case["case_id"], "task": case["prompt"],
            "retrieved_context": contexts}


def _extract_json_array(text: str) -> list[dict]:
    cleaned = text.strip()
    cleaned = re.sub(r"^```(?:json)?\s*", "", cleaned, flags=re.I)
    cleaned = re.sub(r"\s*```$", "", cleaned)
    try:
        value = json.loads(cleaned)
    except json.JSONDecodeError:
        start, end = cleaned.find("["), cleaned.rfind("]")
        if start < 0 or end <= start:
            raise
        value = json.loads(cleaned[start:end + 1])
    if not isinstance(value, list):
        raise ValueError("provider response must be a JSON array")
    return value


ACTOR_SYSTEM = """You are the fixed actor in a memory-delivery experiment.
Answer each engineering task directly and correctly. Apply supplied standing rules only when they
are relevant to that task; do not mention the experiment, the rules, memory, or context. Ignore
irrelevant retrieved material. Return ONLY a JSON array with exactly one object per case:
[{"case_id":"...","answer":"concise answer, at most 90 words"}]."""

JSON_REPAIR_SYSTEM = """Repair the supplied malformed model output into valid JSON. Preserve every
answer's meaning and every expected case_id. Return ONLY a JSON array; do not add or remove cases."""


def run_actor(value_set: dict, treatment: dict, cases: list[dict], retrieval: dict,
              out: Path, resume: bool, batch_size: int = ACTOR_BATCH_SIZE,
              arm_specs: dict[str, dict] | None = None) -> dict:
    defaults = {
        "A": {"policy": "", "source": "A"},
        "T": {"policy": taste_block(value_set), "source": "A"},
        "B": {"policy": "", "source": "B"},
        "C": {"policy": adapt_block(treatment), "source": "A"},
        "D": {"policy": adapt_block(treatment), "source": "B"},
    }
    specs = arm_specs or {arm: defaults[arm] for arm in ARMS}
    blocks = {arm: spec["policy"] for arm, spec in specs.items()}
    actor_dir = out / "actor"
    all_rows = []

    def process_batch(arm: str, batch: list[dict], label: str) -> list[dict]:
        request = {"standing_policy": blocks[arm] or "(none)", "cases": batch}
        request_text = json.dumps(request, ensure_ascii=False)
        if not adapt_sessions.scan_batch_for_secrets_str(request_text):
            raise RuntimeError(f"secret scanner blocked actor arm {arm} batch {label}")
        input_hash = sha_json({"system": ACTOR_SYSTEM, "request": request})
        cache = actor_dir / f"{arm}-{label}.json"
        cached = read_json(cache) if resume and cache.exists() else None
        if isinstance(cached, dict) and cached.get("input_sha256") == input_hash:
            return cached["answers"]

        response = adapt_llm.call_lane_response(
            ACTOR_SYSTEM, request_text, lane="minimax", max_tokens=8000,
            attempts=2, thinking="adaptive", temperature=0.1)
        repair_response = None
        try:
            parsed = _extract_json_array(response["text"])
        except (json.JSONDecodeError, ValueError) as first_error:
            failed = cache.with_suffix(".failed.json")
            write_json(failed, {
                "input_sha256": input_hash, "arm": arm,
                "parse_error": str(first_error), "raw_response": response["text"],
            })
            repair_request = json.dumps({
                "expected_case_ids": [row["case_id"] for row in batch],
                "malformed_output": response["text"],
            }, ensure_ascii=False)
            if not adapt_sessions.scan_batch_for_secrets_str(repair_request):
                raise RuntimeError(f"secret scanner blocked actor JSON repair arm {arm}")
            try:
                repair_response = adapt_llm.call_lane_response(
                    JSON_REPAIR_SYSTEM, repair_request, lane="minimax", max_tokens=8000,
                    attempts=1, thinking="adaptive", temperature=0.0)
                parsed = _extract_json_array(repair_response["text"])
            except Exception as repair_exc:  # provider error or still-malformed repair
                failed_body = read_json(failed)
                failed_body["repair_error"] = str(repair_exc)
                write_json(failed, failed_body)
                if len(batch) <= 5:
                    raise
                midpoint = len(batch) // 2
                return (process_batch(arm, batch[:midpoint], label + "a") +
                        process_batch(arm, batch[midpoint:], label + "b"))

        expected_ids = {row["case_id"] for row in batch}
        actual_ids = {row.get("case_id") for row in parsed}
        if expected_ids != actual_ids or len(parsed) != len(expected_ids):
            if len(batch) > 5:
                midpoint = len(batch) // 2
                return (process_batch(arm, batch[:midpoint], label + "a") +
                        process_batch(arm, batch[midpoint:], label + "b"))
            raise RuntimeError(f"actor coverage mismatch arm={arm} batch={label}")
        write_json(cache, {
            "input_sha256": input_hash, "arm": arm,
            "provider": {k: response.get(k) for k in
                         ("model", "stop_reason", "stop_sequence", "usage")},
            "raw_response": response["text"], "answers": parsed,
            "json_repair": None if repair_response is None else {
                "raw_response": repair_response["text"],
                "provider": {k: repair_response.get(k) for k in
                             ("model", "stop_reason", "stop_sequence", "usage")},
            },
        })
        return parsed

    for arm, spec in specs.items():
        payloads = [arm_case_payload(arm, case, retrieval, spec["source"]) for case in cases]
        for offset in range(0, len(payloads), batch_size):
            batch = payloads[offset:offset + batch_size]
            parsed = process_batch(arm, batch, f"{offset // batch_size + 1:02d}")
            for answer in parsed:
                all_rows.append({"arm": arm, **answer})
    expected = {(arm, case["case_id"]) for arm in specs for case in cases}
    actual = {(row["arm"], row["case_id"]) for row in all_rows}
    if expected != actual or len(all_rows) != len(expected):
        raise RuntimeError("combined actor coverage mismatch")
    result = {"input_sha256": sha_json({"retrieval": retrieval["input_sha256"],
                                         "cases": [r["case_id"] for r in cases]}),
              "answers": all_rows}
    write_json(out / "actor-results.json", result)
    return result


GRADER_SYSTEM = """You are a blind evaluator of engineering-task answers. You do not know which
delivery arm produced an answer. For positive cases, score whether the answer follows the supplied
expected durable rule: adherence 0=miss/contradict, 1=partial, 2=clear. Score task_correct the same
way. For controls, expected_rule is null: set intrusion=true only when the answer imports an
irrelevant forbidden policy; otherwise false. Do not reward wording overlap. Return ONLY a JSON
array with one object per answer: {"answer_id":"...","adherence":0|1|2,
"task_correct":0|1|2,"intrusion":true|false,"rationale":"<=25 words"}."""

STRICT_GRADER_SYSTEM = GRADER_SYSTEM + """
Every field is mandatory. Never return null. For controls, set adherence=0, still score
task_correct 0|1|2, and set intrusion true|false."""


def _expected_rule(case: dict) -> str | None:
    expected = case.get("expected") or {}
    rules = [expected.get("taste_rule"), expected.get("adapt_rule")]
    return "\nEquivalent durable rules:\n- " + "\n- ".join(r for r in rules if r) if any(rules) else None


def valid_grade(row: dict) -> bool:
    return (row.get("adherence") in {0, 1, 2} and
            row.get("task_correct") in {0, 1, 2} and
            isinstance(row.get("intrusion"), bool))


def run_grader(value_set: dict, cases: list[dict], actor: dict, out: Path,
               resume: bool, batch_size: int = GRADER_BATCH_SIZE,
               grader_model: str = DEFAULT_GRADER_MODEL) -> dict:
    case_by_id = {row["case_id"]: row for row in cases}
    concept_rules = {}
    for row in value_set["cases"]:
        if row.get("expected"):
            concept_rules[row["concept"]] = _expected_rule(row)
    blinded = []
    blind_map = {}
    seed = value_set["content_sha256"]
    for row in actor["answers"]:
        answer_id = hashlib.sha256(
            f"{seed}|{row['arm']}|{row['case_id']}".encode()).hexdigest()[:16]
        case = case_by_id[row["case_id"]]
        blind_map[answer_id] = {"arm": row["arm"], "case_id": row["case_id"]}
        blinded.append({
            "answer_id": answer_id,
            "task": case["prompt"],
            "expected_rule": _expected_rule(case),
            "forbidden_rules": [concept_rules.get(name) for name in
                                case.get("forbidden_concepts", []) if concept_rules.get(name)],
            "answer": row["answer"],
        })
    blinded.sort(key=lambda item: item["answer_id"])
    grader_dir = out / "grader"
    grades = []

    def process_batch(batch: list[dict], label: str,
                      system: str = GRADER_SYSTEM) -> list[dict]:
        request_text = json.dumps({"answers": batch}, ensure_ascii=False)
        if not adapt_sessions.scan_batch_for_secrets_str(request_text):
            raise RuntimeError(f"secret scanner blocked grader batch {label}")
        input_hash = sha_json({"system": system, "request": batch,
                               "grader_model": grader_model})
        cache = grader_dir / f"grade-{label}.json"
        failed = grader_dir / f"grade-{label}.failed.json"
        cached = read_json(cache) if resume and cache.exists() else None
        if isinstance(cached, dict) and cached.get("input_sha256") == input_hash:
            return cached["grades"]
        prior_failure = read_json(failed) if resume and failed.exists() else None
        if (isinstance(prior_failure, dict) and
                prior_failure.get("input_sha256") == input_hash and len(batch) > 5):
            midpoint = len(batch) // 2
            return (process_batch(batch[:midpoint], label + "a", system) +
                    process_batch(batch[midpoint:], label + "b", system))

        prompt_file = grader_dir / f"grade-{label}.prompt.json"
        prompt_file.parent.mkdir(parents=True, exist_ok=True)
        prompt_file.write_text(request_text, encoding="utf-8")
        command = [sys.executable, str(API_WORKER), "--provider", "nim",
                   "--model", grader_model, "--input",
                   str(prompt_file), "--system", system,
                   "--max-tokens", "6000"]
        result = subprocess.run(command, capture_output=True, text=True,
                                encoding="utf-8", timeout=600, check=False)
        if result.returncode != 0:
            write_json(failed, {"input_sha256": input_hash, "provider_error": result.stderr[-2000:]})
            raise RuntimeError(f"grader failed batch {label}: {result.stderr[-1000:]}")
        try:
            parsed = _extract_json_array(result.stdout)
            expected_ids = {row["answer_id"] for row in batch}
            actual_ids = {row.get("answer_id") for row in parsed}
            if expected_ids != actual_ids or len(parsed) != len(expected_ids):
                raise ValueError(
                    f"coverage expected={len(expected_ids)} actual={len(actual_ids)}")
            if system == STRICT_GRADER_SYSTEM and any(not valid_grade(row) for row in parsed):
                raise ValueError("strict grader returned null or out-of-range score fields")
        except (json.JSONDecodeError, ValueError) as exc:
            write_json(failed, {"input_sha256": input_hash, "parse_error": str(exc),
                                "raw_response": result.stdout})
            if len(batch) <= 5:
                raise
            midpoint = len(batch) // 2
            return (process_batch(batch[:midpoint], label + "a", system) +
                    process_batch(batch[midpoint:], label + "b", system))
        write_json(cache, {"input_sha256": input_hash,
                           "raw_response": result.stdout, "grades": parsed})
        return parsed

    for offset in range(0, len(blinded), batch_size):
        batch = blinded[offset:offset + batch_size]
        parsed = process_batch(batch, f"{offset // batch_size + 1:02d}")
        grades.extend(parsed)

    invalid_ids = {row.get("answer_id") for row in grades if not valid_grade(row)}
    if invalid_ids:
        invalid_batch = [row for row in blinded if row["answer_id"] in invalid_ids]
        repaired = process_batch(
            invalid_batch, "invalid-" + sha_json(sorted(invalid_ids))[:8],
            STRICT_GRADER_SYSTEM)
        repaired_by_id = {row["answer_id"]: row for row in repaired}
        grades = [repaired_by_id.get(row.get("answer_id"), row) for row in grades]
    remaining_invalid = [row for row in grades if not valid_grade(row)]
    if remaining_invalid:
        raise RuntimeError(f"grader returned {len(remaining_invalid)} invalid score rows")
    enriched = [{**blind_map[row["answer_id"]], **row} for row in grades]
    result = {"grader": f"nim/{grader_model}",
              "blind_map_sha256": sha_json(blind_map), "grades": enriched}
    write_json(out / "grader-results.json", result)
    return result


def _mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def _exact_mcnemar_p(left_only: int, right_only: int) -> float:
    n = left_only + right_only
    if n == 0:
        return 1.0
    tail = min(left_only, right_only)
    probability = sum(__import__("math").comb(n, k) for k in range(tail + 1)) / (2 ** n)
    return min(1.0, 2 * probability)


def paired_arm_stats(grades: list[dict], cases: list[dict], left: str,
                     right: str, *, samples: int = 20000) -> dict:
    positive_ids = {row["case_id"] for row in cases if row["cohort"] != "control"}
    scores = {(row["arm"], row["case_id"]): row for row in grades
              if row["case_id"] in positive_ids}
    diffs = []
    left_only = right_only = 0
    for case_id in sorted(positive_ids):
        lrow, rrow = scores[(left, case_id)], scores[(right, case_id)]
        diffs.append((rrow["adherence"] - lrow["adherence"]) / 2 * 100)
        lfull, rfull = lrow["adherence"] == 2, rrow["adherence"] == 2
        left_only += int(lfull and not rfull)
        right_only += int(rfull and not lfull)
    rng = random.Random(20260714 + sum(map(ord, left + right)))
    means = sorted(_mean([diffs[rng.randrange(len(diffs))] for _ in diffs])
                   for _ in range(samples))
    return {
        "left": left, "right": right,
        "mean_adherence_delta_pp": round(_mean(diffs), 1),
        "bootstrap_95_ci_pp": [round(means[int(samples * 0.025)], 1),
                               round(means[int(samples * 0.975)], 1)],
        "full_adherence_discordance": {
            "left_only": left_only, "right_only": right_only,
            "discordant": left_only + right_only,
            "mcnemar_exact_p": round(_exact_mcnemar_p(left_only, right_only), 6),
        },
    }


def analyze(value_set: dict, cases: list[dict], retrieval: dict, actor: dict,
            grader: dict, out: Path) -> dict:
    case_by_id = {row["case_id"]: row for row in cases}
    audited_controls = control_audit.audit(actor)
    audited_by_key = {(row["arm"], row["case_id"]): row
                      for row in audited_controls["rows"]}
    write_json(out / "control-audit.json", audited_controls)
    metrics = {}
    for arm in ARMS:
        rows = [row for row in grader["grades"] if row["arm"] == arm]
        positives = [row for row in rows if case_by_id[row["case_id"]]["cohort"] != "control"]
        controls = [row for row in rows if case_by_id[row["case_id"]]["cohort"] == "control"]
        by_cohort = {}
        for cohort in ("shared", "taste_only", "adapt_only"):
            subset = [row for row in positives if case_by_id[row["case_id"]]["cohort"] == cohort]
            by_cohort[cohort] = round(100 * _mean([row["adherence"] / 2 for row in subset]), 1)
        source = retrieval["B"] if arm in {"B", "D"} else retrieval["A"]
        context_chars = [sum(len(t) for t in _safe_context(source[row["case_id"]]))
                         for row in cases]
        metrics[arm] = {
            "adherence_pct": round(100 * _mean([row["adherence"] / 2 for row in positives]), 1),
            "full_adherence_pct": round(100 * _mean([row["adherence"] == 2 for row in positives]), 1),
            "task_correct_pct": round(100 * _mean([row["task_correct"] / 2 for row in rows]), 1),
            "control_intrusion_pct": round(100 * _mean([
                audited_by_key[(row["arm"], row["case_id"])]["intrusion"]
                for row in controls]), 1),
            "grader_control_intrusion_pct": round(
                100 * _mean([bool(row["intrusion"]) for row in controls]), 1),
            "by_cohort_adherence_pct": by_cohort,
            "mean_retrieved_context_chars": round(_mean(context_chars), 1),
            "loaded_policy_chars": len(taste_block(value_set)) if arm == "T" else
                                   len(adapt_block(read_json(out / "frozen/adapt-treatment.json")))
                                   if arm in {"C", "D"} else 0,
        }
    baseline = metrics["A"]
    for arm in ARMS:
        metrics[arm]["adherence_delta_vs_A_pp"] = round(
            metrics[arm]["adherence_pct"] - baseline["adherence_pct"], 1)
    best_adapt = max(("B", "C", "D"), key=lambda arm: (
        metrics[arm]["adherence_pct"], -metrics[arm]["control_intrusion_pct"]))
    taste = metrics["T"]
    best = metrics[best_adapt]
    parity_gap = round(best["adherence_pct"] - taste["adherence_pct"], 1)
    parity = parity_gap >= -5 and best["control_intrusion_pct"] <= taste["control_intrusion_pct"] + 5
    useful = (best["adherence_delta_vs_A_pp"] >= 15 and
              best["control_intrusion_pct"] <= baseline["control_intrusion_pct"] + 5)
    result = {
        "design": "five-arm-delivery-parity-v1", "case_count": len(cases),
        "control_intrusion_method": audited_controls["method"],
        "adapt_treatment": read_json(out / "frozen/adapt-treatment.json").get("variant", "raw"),
        "metrics": metrics, "best_adapt_arm": best_adapt,
        "best_adapt_minus_taste_pp": parity_gap,
        "taste_parity_within_5pp_and_no_material_intrusion": parity,
        "adapt_value_gate_15pp_and_no_material_intrusion": useful,
        "paired_comparisons": {
            f"{left}_vs_{right}": paired_arm_stats(grader["grades"], cases, left, right)
            for left, right in (("A", "T"), ("A", "D"), ("T", "D"),
                                ("B", "D"), ("C", "D"))
        },
        "limitations": [
            "Synthetic artifact-derived challenge set; no Adrian-authored labels.",
            "One fixed MiniMax M3 actor and one fixed blinded DeepSeek V4 Pro grader.",
            "Always-loaded arms carry their full digest while retrieval arms carry top-3 memories.",
        ],
    }
    write_json(out / "analysis.json", result)
    lines = ["# Taste vs Adapt delivery parity", "",
             f"- cases: {len(cases)}", f"- best Adapt arm: **{best_adapt}**",
             f"- best Adapt minus Taste: **{parity_gap:+.1f} pp**",
             f"- Taste parity: **{'PASS' if parity else 'FAIL'}**",
             f"- Adapt value gate: **{'PASS' if useful else 'FAIL'}**", "",
             "| Arm | Adherence | Full | Correct | Intrusion | Shared | Taste-only | Adapt-only | Loaded chars | Retrieved chars |",
             "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for arm in ARMS:
        m = metrics[arm]
        lines.append(f"| {arm} | {m['adherence_pct']:.1f}% | {m['full_adherence_pct']:.1f}% | "
                     f"{m['task_correct_pct']:.1f}% | {m['control_intrusion_pct']:.1f}% | "
                     f"{m['by_cohort_adherence_pct']['shared']:.1f}% | "
                     f"{m['by_cohort_adherence_pct']['taste_only']:.1f}% | "
                     f"{m['by_cohort_adherence_pct']['adapt_only']:.1f}% | "
                     f"{m['loaded_policy_chars']} | {m['mean_retrieved_context_chars']:.0f} |")
    lines += ["", "## Paired comparisons", "",
              "| Comparison | Delta | Bootstrap 95% CI | Full-only discordance | McNemar p |",
              "|---|---:|---:|---:|---:|"]
    for name, stat in result["paired_comparisons"].items():
        disc = stat["full_adherence_discordance"]
        ci = stat["bootstrap_95_ci_pp"]
        lines.append(f"| {name} | {stat['mean_adherence_delta_pp']:+.1f} pp | "
                     f"[{ci[0]:+.1f}, {ci[1]:+.1f}] | "
                     f"{disc['left_only']} left / {disc['right_only']} right | "
                     f"{disc['mcnemar_exact_p']:.6f} |")
    lines += ["", "A=Cortex; T=A+CommandCode Taste; B=Adapt retrieved; "
              "C=Adapt digest loaded; D=Adapt digest+retrieval.", "",
              "Raw retrieval, actor responses, blind grades, hashes, and provider metadata are beside this report."]
    (out / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return result


def _freeze(out: Path, taste: Path, adapt: Path,
            variant: str = "raw") -> tuple[dict, dict]:
    value_set, source_treatment = builder.build_value_set(taste, adapt)
    treatment = select_adapt_treatment(source_treatment, variant)
    frozen = out / "frozen"
    write_json(frozen / "value-set.json", value_set)
    write_json(frozen / "adapt-treatment-source.json", source_treatment)
    write_json(frozen / "adapt-treatment.json", treatment)
    return value_set, treatment


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", choices=("retrieval", "actor", "grade", "analyze", "all"),
                        default="all")
    parser.add_argument("--smoke", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--taste", type=Path, default=builder.DEFAULT_TASTE)
    parser.add_argument("--adapt", type=Path, default=builder.DEFAULT_ADAPT)
    parser.add_argument("--adapt-variant", choices=("raw", "curated"), default="raw")
    parser.add_argument("--grader-model", default=DEFAULT_GRADER_MODEL)
    parser.add_argument("--live-db", type=Path, default=DEFAULT_LIVE_DB)
    parser.add_argument("--membrane-bin", type=Path, default=DEFAULT_MEMBRANE)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args(argv)
    out = args.out or (ROOT / ".cache/adapt-delivery-parity" /
                       ("smoke" if args.smoke else "full"))
    out.mkdir(parents=True, exist_ok=True)
    value_set, treatment = _freeze(out, args.taste, args.adapt, args.adapt_variant)
    cases = select_cases(value_set["cases"], args.smoke)
    write_json(out / "selected-cases.json", {"smoke": args.smoke, "cases": cases})

    retrieval_path, actor_path, grader_path = (
        out / "retrieval.json", out / "actor-results.json", out / "grader-results.json")
    retrieval = actor = grader = None
    if args.stage in {"retrieval", "all"}:
        print(f"retrieval: {len(cases)} cases, two copied DB arms")
        retrieval = run_retrieval(value_set, treatment, cases, out, args.live_db,
                                  args.membrane_bin, args.resume)
    elif retrieval_path.exists():
        retrieval = read_json(retrieval_path)
    if args.stage in {"actor", "all"}:
        if retrieval is None:
            raise RuntimeError("retrieval.json required before actor stage")
        print(f"actor: {len(cases) * len(ARMS)} blinded-arm answers via MiniMax M3")
        actor = run_actor(value_set, treatment, cases, retrieval, out, args.resume)
    elif actor_path.exists():
        actor = read_json(actor_path)
    if args.stage in {"grade", "all"}:
        if actor is None:
            raise RuntimeError("actor-results.json required before grade stage")
        print(f"grader: {len(actor['answers'])} answers via fixed blinded {args.grader_model}")
        grader = run_grader(value_set, cases, actor, out, args.resume,
                            grader_model=args.grader_model)
    elif grader_path.exists():
        grader = read_json(grader_path)
    if args.stage in {"analyze", "all"}:
        if retrieval is None or actor is None or grader is None:
            raise RuntimeError("retrieval, actor, and grader results required for analysis")
        result = analyze(value_set, cases, retrieval, actor, grader, out)
        print(json.dumps(result, indent=2))
        print(out / "report.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
