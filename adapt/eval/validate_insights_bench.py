"""Validate the P0.5 portable labelled Insights benchmark locally.

Checks ``adapt/eval/insights_bench/v1/{cases.jsonl,manifest.json}``:

1. every line is valid JSON and conforms to
   ``adapt/eval/insights_bench_case.schema.json`` (jsonschema when importable,
   otherwise an equivalent built-in structural check);
2. payload_sha256 seals exactly the canonical serialization of the immutable
   payload, and case_id derives from it (canonical 7.3/7.4);
3. byte spans reconstruct the excerpt stream contiguously with correct UTF-8
   lengths, and source_digests match the concatenated event texts;
4. the manifest's corpus/schema digests, per-case index, label/class/family
   counts, and P0.5 coverage requirements all hold.

Exits non-zero on any failure. No network, no model calls.
"""
from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

EVAL_DIR = Path(__file__).resolve().parent
BENCH_DIR = EVAL_DIR / "insights_bench" / "v1"
CASES_PATH = BENCH_DIR / "cases.jsonl"
MANIFEST_PATH = BENCH_DIR / "manifest.json"
SCHEMA_PATH = EVAL_DIR / "insights_bench_case.schema.json"

ROLE_TAGS = {"u", "a", "tc", "tr"}
SEVERITIES = {"info", "low", "medium", "high", "critical"}

failures: list[str] = []
checks = 0


def check(cond: bool, msg: str) -> None:
    global checks
    checks += 1
    if not cond:
        failures.append(msg)


def canonical_payload_bytes(payload: dict) -> bytes:
    return json.dumps(payload, sort_keys=True, ensure_ascii=False,
                      separators=(",", ":")).encode("utf-8")


def structural_case_check(case: dict, where: str) -> None:
    """Schema-equivalent checks that also run without jsonschema."""
    for key in ("schema_version", "bench_id", "case_id", "payload_sha256",
                "payload", "state"):
        check(key in case, f"{where}: missing required field {key}")
    if set(case) - {"schema_version", "bench_id", "case_id", "payload_sha256",
                    "payload", "state"}:
        check(False, f"{where}: unexpected top-level fields")
    check(case.get("schema_version") == "1.0.0",
          f"{where}: schema_version must be '1.0.0'")
    check(re.fullmatch(r"adapt-insights-bench-[a-f0-9]{12}",
                       case.get("bench_id", "")) is not None,
          f"{where}: bad bench_id pattern")
    check(re.fullmatch(r"ibc_[a-f0-9]{64}", case.get("case_id", "")) is not None,
          f"{where}: bad case_id pattern")
    check(re.fullmatch(r"[a-f0-9]{64}", case.get("payload_sha256", "")) is not None,
          f"{where}: bad payload_sha256 pattern")

    p = case.get("payload") or {}
    req = ["record_kind", "family", "label", "case_class", "transcript_excerpt",
           "source_digests", "expected", "honesty_limit",
           "admission_policy_version", "redaction_contract_version"]
    for key in req:
        check(key in p, f"{where}.payload: missing {key}")
    check(set(p) - set(req) == set(), f"{where}.payload: unexpected fields")
    check(p.get("record_kind") == "insights_bench_case",
          f"{where}.payload.record_kind wrong")
    check(p.get("label") in ("positive", "negative"), f"{where}: bad label")
    check(p.get("case_class") in {
        "real_failure", "negated", "quoted_context_carried",
        "tool_result_text", "hypothetical_narration",
        "cross_session_duplicate"}, f"{where}: bad case_class")
    check(isinstance(p.get("honesty_limit"), str) and p["honesty_limit"],
          f"{where}: honesty_limit must be a non-empty string")

    ex = p.get("transcript_excerpt") or {}
    check(set(ex) == {"session_id", "events"}, f"{where}: bad excerpt keys")
    events = ex.get("events") or []
    check(len(events) >= 1, f"{where}: needs >=1 event")
    offset = 0
    stream = b""
    for i, ev in enumerate(events):
        w = f"{where}.event[{i}]"
        check(set(ev) == {"event_id", "session_id", "kind", "role", "byte_start", "byte_end", "text"},
              f"{w}: bad event keys")
        check(ev.get("kind") in {"user_message", "assistant_message", "tool_call", "tool_result"},
              f"{w}: invalid kind")
        check(ev.get("role") in {"user", "assistant", "tool"}, f"{w}: invalid role")
        check(isinstance(ev.get("session_id"), str) and bool(ev.get("session_id")),
              f"{w}: invalid session_id")
        text = ev.get("text", "")
        data = text.encode("utf-8")
        check(ev.get("byte_start") == offset, f"{w}: non-contiguous byte_start")
        check(ev.get("byte_end") == offset + len(data),
              f"{w}: byte_end != byte_start + utf8(text)")
        check(len(data) > 0, f"{w}: empty text")
        tag = ev.get("event_id", "")
        check(tag.endswith(tuple(ROLE_TAGS)) or tag[-2:] in ("tc", "tr")
              or tag[-1] in ROLE_TAGS, f"{w}: event_id lacks role tag")
        offset += len(data)
        stream += data

    for d in p.get("source_digests") or []:
        check(re.fullmatch(r"sha256:[a-f0-9]{64}", d) is not None,
              f"{where}: bad source_digest format")
    if p.get("source_digests"):
        expect = "sha256:" + hashlib.sha256(stream).hexdigest()
        check(p["source_digests"][0] == expect,
              f"{where}: source_digest does not match excerpt bytes")

    exp = p.get("expected") or {}
    check(set(exp) - {"detected", "family_match", "min_severity",
                      "confidence_ceiling"} == set(),
          f"{where}.expected: unexpected fields")
    check(isinstance(exp.get("detected"), bool), f"{where}: detected must be bool")
    check((p.get("label") == "positive") == exp.get("detected"),
          f"{where}: label/detected mismatch")
    check(isinstance(exp.get("family_match"), str) and exp["family_match"],
          f"{where}: family_match must be a non-empty string")
    if "min_severity" in exp:
        check(exp["min_severity"] in SEVERITIES, f"{where}: bad min_severity")
    if "confidence_ceiling" in exp:
        cc = exp["confidence_ceiling"]
        check(isinstance(cc, (int, float)) and 0 <= cc <= 1,
              f"{where}: confidence_ceiling out of range")

    st = case.get("state") or {}
    check(set(st) == {"review_status", "updated_at", "receipts"},
          f"{where}.state: bad keys")
    check(st.get("review_status") in ("draft", "reviewed", "frozen", "retired"),
          f"{where}: bad review_status")
    check(re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z",
                       st.get("updated_at", "")) is not None,
          f"{where}: updated_at must be ISO-8601 Z timestamp")
    for i, r in enumerate(st.get("receipts") or []):
        w = f"{where}.receipt[{i}]"
        check({"transition", "at", "actor", "prev_status", "new_status",
               "receipt_id"} <= set(r), f"{w}: missing receipt fields")
        check(re.fullmatch(r"rcpt_[a-f0-9]{32}", r.get("receipt_id", ""))
              is not None, f"{w}: bad receipt_id")


def main() -> int:
    print(f"validating {CASES_PATH.relative_to(EVAL_DIR.parent.parent)}")

    # -- load -----------------------------------------------------------------
    try:
        raw = CASES_PATH.read_bytes()
    except OSError as exc:
        print(f"FATAL: cannot read cases: {exc}")
        return 1
    cases = []
    for n, line in enumerate(raw.decode("utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            cases.append(json.loads(line))
        except json.JSONDecodeError as exc:
            failures.append(f"line {n}: invalid JSON: {exc}")
    print(f"parsed {len(cases)} cases")

    # -- schema ---------------------------------------------------------------
    try:
        import jsonschema  # type: ignore
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        validator = jsonschema.Draft7Validator(schema)
        schema_mode = "jsonschema Draft7"
        for i, case in enumerate(cases):
            for err in validator.iter_errors(case):
                failures.append(f"case[{i}]: schema violation at "
                                f"{'/'.join(map(str, err.absolute_path))}: "
                                f"{err.message}")
    except ImportError:
        schema_mode = "built-in structural (jsonschema unavailable)"
        for i, case in enumerate(cases):
            structural_case_check(case, f"case[{i}]")
    print(f"schema mode: {schema_mode}")

    # -- sealing & identity (canonical 7.3/7.4) ---------------------------------
    for i, case in enumerate(cases):
        where = f"case[{i}] ({case.get('case_id', '?')[:20]}...)"
        digest = hashlib.sha256(
            canonical_payload_bytes(case["payload"])).hexdigest()
        check(case["payload_sha256"] == digest,
              f"{where}: payload_sha256 does not seal canonical payload")
        check(case["case_id"] == "ibc_" + case["payload_sha256"],
              f"{where}: case_id not derived from payload_sha256")

    # -- manifest ---------------------------------------------------------------
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    corpus_digest = hashlib.sha256(raw).hexdigest()
    check(manifest["corpus"]["sha256"] == corpus_digest,
          "manifest corpus sha256 mismatch")
    check(manifest["corpus"]["byte_size"] == len(raw),
          "manifest corpus byte_size mismatch")
    check(manifest["corpus"]["case_count"] == len(cases),
          "manifest case_count mismatch")
    schema_digest = hashlib.sha256(SCHEMA_PATH.read_bytes()).hexdigest()
    check(manifest["case_schema"]["sha256"] == schema_digest,
          "manifest case_schema digest mismatch")
    seed = "|".join(["p0.5", "v1", manifest["schema_version"],
                     manifest["created_at"],
                     ",".join(manifest["required_coverage"]["families"])])
    check(manifest["bench_id"] ==
          "adapt-insights-bench-" + hashlib.sha256(seed.encode()).hexdigest()[:12],
          "manifest bench_id does not follow its declared derivation rule")

    index = {c["case_id"]: c for c in manifest["cases"]}
    check(len(index) == len(manifest["cases"]), "duplicate case ids in manifest")
    fam_counts: dict[str, dict[str, int]] = {}
    cls_counts: dict[str, dict[str, int]] = {}
    lab_counts = {"positive": 0, "negative": 0}
    seen_ids: set[str] = set()
    for case in cases:
        cid = case["case_id"]
        check(cid not in seen_ids, f"duplicate case_id in corpus: {cid[:24]}")
        seen_ids.add(cid)
        entry = index.get(cid)
        check(entry is not None, f"case {cid[:24]} missing from manifest index")
        if entry:
            check(entry["payload_sha256"] == case["payload_sha256"],
                  f"{cid[:24]}: manifest payload digest mismatch")
        p = case["payload"]
        fam_counts.setdefault(p["family"], {"positive": 0, "negative": 0})[
            p["label"]] += 1
        cls_counts.setdefault(p["case_class"], {"positive": 0, "negative": 0})[
            p["label"]] += 1
        lab_counts[p["label"]] += 1
    check(set(index) == seen_ids, "manifest index contains unknown case ids")

    check(manifest["families"] == fam_counts, "manifest family counts mismatch")
    check(manifest["case_classes"] == cls_counts, "manifest class counts mismatch")
    check(manifest["labels"] == lab_counts, "manifest label counts mismatch")

    # -- P0.5 coverage requirements ----------------------------------------------
    required_fams = manifest["required_coverage"]["families"]
    for fam in required_fams:
        got = fam_counts.get(fam, {})
        check(got.get("positive", 0) >=
              manifest["required_coverage"]["min_positive_per_family"],
              f"family {fam}: no positive case")
    present = set(fam_counts)
    check(present == set(required_fams),
          f"unexpected families present: {sorted(present - set(required_fams))}"
          f" / missing: {sorted(set(required_fams) - present)}")
    for cls in manifest["required_coverage"]["classes_requiring_negative"]:
        check(cls_counts.get(cls, {}).get("negative", 0) >= 1,
              f"case_class {cls}: no negative (trap) case")
    for cls in ("real_failure",):
        check(cls_counts.get(cls, {}).get("positive", 0) >= 1,
              f"case_class {cls}: no positive case")

    # -- report --------------------------------------------------------------------
    print("\nfamily coverage (positive/negative):")
    for fam in sorted(fam_counts):
        c = fam_counts[fam]
        mark = "" if c["positive"] >= 1 else "  <-- NO POSITIVE"
        print(f"  {fam:42s} {c['positive']}/{c['negative']}{mark}")
    print("\ncase classes (positive/negative):")
    for cls in sorted(cls_counts):
        c = cls_counts[cls]
        print(f"  {cls:28s} {c['positive']}/{c['negative']}")
    print(f"\nlabels: {lab_counts}")

    if failures:
        print(f"\nFAIL: {len(failures)} problem(s) out of {checks} checks:")
        for f in failures[:40]:
            print(f"  - {f}")
        return 1
    print(f"\nPASS: all {checks} checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
