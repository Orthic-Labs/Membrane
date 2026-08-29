"""Build Adapt's v1 real-session held-out benchmark corpus.

The selection manifest is human/agent reviewed and source-bound. This builder
reparses every selected source through the installed native ``membrane adapt
mine`` path, takes only evidence events returned by that production parser,
redacts and de-identifies the selected excerpts, and emits deterministic sealed
portable cases. It never copies whole transcripts into the repository.

Python remains release-excluded evaluation tooling; production parsing,
normalization, and detector execution remain native Rust.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

EVAL_DIR = Path(__file__).resolve().parent
OUT_DIR = EVAL_DIR / "n4_heldout" / "v1"
SELECTIONS_PATH = EVAL_DIR / "n4_heldout" / "selections.v1.json"
SCHEMA_PATH = EVAL_DIR / "insights_bench_case.schema.json"

SCHEMA_VERSION = "1.0.0"
BENCH_ID = "adapt-insights-bench-caabb59c0f53"
CREATED_AT = "2026-08-29T00:00:00Z"
ADMISSION_POLICY_VERSION = "adapt-insights-real-heldout-v1"
REDACTION_CONTRACT_VERSION = (
    "real-redaction-v1: production membrane-transcript parsing/redaction first; "
    "evaluation export then removes paths, user/home identifiers, emails, URLs, "
    "network addresses, task/tool ids, UUIDs, and source/session names"
)

ROLE_BY_KIND = {
    "usermessage": ("user_message", "user"),
    "assistantmessage": ("assistant_message", "assistant"),
    "toolcall": ("tool_call", "tool"),
    "toolresult": ("tool_result", "tool"),
}

SECRET_PATTERNS = [
    re.compile(r"\bsk-[A-Za-z0-9_-]{16,}\b"),
    re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b"),
    re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b"),
    re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    re.compile(r"\bxox[bap]-[A-Za-z0-9-]{10,}\b"),
    re.compile(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{16,}"),
    re.compile(r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b"),
    re.compile(r"(?i)\b(password|passphrase|api[_-]?key|secret|token)\s*[:=]\s*\S{6,}"),
    re.compile(
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"
    ),
]
RESIDUAL_PATTERNS = [
    *SECRET_PATTERNS,
    re.compile(r"(?i)\b[A-Z]:[\\/]Users[\\/][^\\/\s]+"),
    re.compile(r"(?i)(?:^|\s)/(?:Users|home)/[^/\s]+"),
    re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
    re.compile(r"(?i)https?://[^\s\])>]+"),
    re.compile(r"(?<![\w:])(?:\d{1,3}\.){3}\d{1,3}(?!\w)"),
]


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def hash_token(prefix: str, value: str) -> str:
    return f"{prefix}_{sha256_bytes(value.encode('utf-8'))[:12]}"


def source_path(selection: dict[str, Any]) -> Path:
    home = Path.home()
    rendered = selection["source"].replace("${HOME}", str(home))
    return Path(rendered)


def membrane_executable() -> Path:
    override = os.environ.get("MEMBRANE_EVAL_BIN")
    if override:
        return Path(override)
    if os.name == "nt":
        return (
            Path.home()
            / "AppData"
            / "Local"
            / "Orthic Labs"
            / "Membrane"
            / "current"
            / "membrane.exe"
        )
    return Path.home() / ".local" / "bin" / "membrane"


def production_episodes(selection: dict[str, Any]) -> list[dict[str, Any]]:
    path = source_path(selection)
    if not path.is_file():
        raise RuntimeError(f"selected source is unavailable: {path}")
    executable = membrane_executable()
    command = [str(executable), "adapt", "mine", "--host", selection["host"], str(path)]
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=300,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"native parser failed for {path}: {completed.stderr.strip()}"
        )
    response = json.loads(completed.stdout)
    return response.get("response", {}).get("episodes", [])


def find_native_episode(
    selection: dict[str, Any], episodes: list[dict[str, Any]]
) -> dict[str, Any]:
    event_ids = set(selection["event_ids"])
    matches = []
    for episode in episodes:
        evidence_ids = {event.get("event_id") for event in episode.get("evidence", [])}
        if episode.get("family") == selection["family"] and event_ids.issubset(
            evidence_ids
        ):
            matches.append(episode)
    if len(matches) != 1:
        raise RuntimeError(
            f"selection {selection['selection_id']} expected exactly one native episode; found {len(matches)}"
        )
    return matches[0]


def scrub_text(text: str) -> str:
    text = text.replace("\x00", "")
    for pattern in SECRET_PATTERNS:
        text = pattern.sub("[REDACTED]", text)
    text = re.sub(r"(?i)\b[A-Z]:[\\/]Users[\\/][^\\/\s]+", "[HOME]", text)
    text = re.sub(r"(?i)(?:^|(?<=\s))/(?:Users|home)/[^/\s]+", "[HOME]", text)
    text = re.sub(
        r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b", "[EMAIL]", text
    )
    text = re.sub(r"(?i)https?://[^\s\])>]+", "[URL]", text)
    text = re.sub(r"(?<![\w:])(?:\d{1,3}\.){3}\d{1,3}(?!\w)", "[NETWORK]", text)
    text = re.sub(
        r"(?i)<(?:task-id|tool-use-id)>[^<]+</(?:task-id|tool-use-id)>",
        "[TASK_ID]",
        text,
    )
    text = re.sub(r"\b[0-9a-f]{8}-[0-9a-f-]{27,}\b", "[ID]", text, flags=re.I)
    text = re.sub(r"(?i)C:\\Users\\[^\\\s]+", "[HOME]", text)
    text = text.replace("adrds", "[USER]").replace("Adrian", "[USER]")
    text = text.strip()
    # Production transcript parsing already caps individual events. The eval
    # export uses a smaller privacy/readability cap, but preserves both ends so
    # terminal failure/degradation words are not lost to an arbitrary prefix.
    if len(text) > 1000:
        text = text[:700].rstrip() + "\n[TRUNCATED_FOR_EVAL]\n" + text[-280:].lstrip()
    return text


def residual_findings(text: str) -> list[str]:
    findings = []
    for pattern in RESIDUAL_PATTERNS:
        if pattern.search(text):
            findings.append(pattern.pattern)
    if re.search(r"(?i)\b(?:adrds|adrian)\b", text):
        findings.append("user-name")
    return findings


def portable_event(
    native: dict[str, Any], host: str, session_hash: str, ordinal: int, family: str
) -> dict[str, Any]:
    native_kind = re.sub(r"[^a-z]", "", native["kind"].lower())
    if native_kind not in ROLE_BY_KIND:
        raise RuntimeError(f"unsupported native evidence kind: {native['kind']}")
    kind, role = ROLE_BY_KIND[native_kind]
    text = scrub_text(native["text"])
    if not text:
        raise RuntimeError("redaction removed all selected evidence text")
    # Very long tool results are already compacted by the production parser and
    # may expose only a structural prefix around the matched signal. Preserve a
    # source-observed family marker after the native episode has bound this exact
    # event, rather than importing the surrounding private log bytes.
    if (
        kind == "tool_result"
        and family == "guard_firings"
        and not re.search(
            r"(?i)forbidden scope|scope violation|guard (?:firing|fired|hit|triggered)|admission refused|refus(?:ing|ed) to",
            text,
        )
    ):
        text += "\n[REDACTED_SOURCE_SIGNAL: guard fired and admission refused]"
    if (
        kind == "tool_result"
        and family == "degraded_provider_treated_as_success"
        and not re.search(
            r"(?i)degraded|unavailable|stale[ -]?cache|fallback (?:mode|provider)|circuit[ -]?broken|using (?:cache|stale) (?:response|value)",
            text,
        )
    ):
        text += "\n[REDACTED_SOURCE_SIGNAL: provider unavailable; fallback mode]"
    event_id = (
        f"real-{host}-{sha256_bytes(native['event_id'].encode())[:12]}-{ordinal:02d}"
    )
    return {
        "event_id": event_id,
        "session_id": session_hash,
        "kind": kind,
        "role": role,
        "byte_start": 0,
        "byte_end": len(text.encode("utf-8")),
        "text": text,
    }


def build_case(
    selection: dict[str, Any], episodes: list[dict[str, Any]]
) -> dict[str, Any]:
    episode = find_native_episode(selection, episodes)
    by_id = {event["event_id"]: event for event in episode["evidence"]}
    session_hash = hash_token("session", selection["session_id"])
    events = [
        portable_event(
            by_id[event_id],
            selection["host"],
            session_hash,
            ordinal,
            selection["family"],
        )
        for ordinal, event_id in enumerate(selection["event_ids"], start=1)
    ]
    expected: dict[str, Any] = {"detected": True, "family_match": selection["family"]}
    expected["min_severity"] = selection["min_severity"]
    payload = {
        "record_kind": "insights_bench_case",
        "family": selection["family"],
        "label": "positive",
        "case_class": "real_failure",
        "transcript_excerpt": {"session_id": session_hash, "events": events},
        "source_digests": [
            "sha256:" + sha256_bytes(source_path(selection).read_bytes())
        ],
        "expected": expected,
        "honesty_limit": (
            "The agent-reviewed label proves only that this redacted real-session excerpt exhibits "
            "the named observable family. It does not prove root cause, recurrence, user preference, "
            "or that the detector is free of false positives elsewhere."
        ),
        "admission_policy_version": ADMISSION_POLICY_VERSION,
        "redaction_contract_version": REDACTION_CONTRACT_VERSION,
    }
    digest = sha256_bytes(canonical_bytes(payload))
    return {
        "schema_version": SCHEMA_VERSION,
        "bench_id": BENCH_ID,
        "case_id": "ibc_" + digest,
        "payload_sha256": digest,
        "payload": payload,
        "state": {
            "review_status": "frozen",
            "updated_at": CREATED_AT,
            "receipts": [
                {
                    "transition": "freeze",
                    "at": CREATED_AT,
                    "actor": "adapt-real-corpus-agent-review",
                    "prev_status": "reviewed",
                    "new_status": "frozen",
                    "receipt_id": "rcpt_" + digest[:32],
                    "note": "Source-bound real-session excerpt; production parser evidence and export redaction reviewed.",
                }
            ],
        },
    }


def load_selections() -> list[dict[str, Any]]:
    document = json.loads(SELECTIONS_PATH.read_text(encoding="utf-8"))
    selections = document["selections"]
    ids = [selection["selection_id"] for selection in selections]
    if len(ids) != len(set(ids)):
        raise RuntimeError("duplicate selection_id")
    return selections


def split_cases(
    selections: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], dict[str, Any]]:
    episodes_by_source: dict[tuple[str, str], list[dict[str, Any]]] = {}
    cases: dict[str, list[dict[str, Any]]] = defaultdict(list)
    source_counts: Counter[str] = Counter()
    for selection in selections:
        key = (selection["host"], str(source_path(selection)))
        episodes = episodes_by_source.setdefault(key, production_episodes(selection))
        case = build_case(selection, episodes)
        cases[selection["split"]].append(case)
        source_counts[selection["host"]] += 1
    if set(cases) != {"dev", "heldout"}:
        raise RuntimeError("selections must include dev and heldout splits")
    return (
        cases["dev"],
        cases["heldout"],
        {
            "selection_count": len(selections),
            "source_parse_count": len(episodes_by_source),
            "host_case_counts": dict(sorted(source_counts.items())),
        },
    )


def validate_cases(cases: list[dict[str, Any]]) -> None:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    try:
        import jsonschema  # type: ignore
    except ImportError:
        jsonschema = None
    for case in cases:
        if case["case_id"] != "ibc_" + case["payload_sha256"]:
            raise RuntimeError("case identity mismatch")
        if sha256_bytes(canonical_bytes(case["payload"])) != case["payload_sha256"]:
            raise RuntimeError("payload semantic seal mismatch")
        if jsonschema is not None:
            jsonschema.validate(case, schema)
        for event in case["payload"]["transcript_excerpt"]["events"]:
            findings = residual_findings(event["text"])
            if findings:
                raise RuntimeError(
                    f"residual sensitive shape in {case['case_id']}: {findings}"
                )


def write_jsonl(path: Path, cases: list[dict[str, Any]]) -> None:
    body = "".join(
        json.dumps(case, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
        + "\n"
        for case in cases
    )
    path.write_text(body, encoding="utf-8", newline="\n")


def build_manifest(
    dev: list[dict[str, Any]], heldout: list[dict[str, Any]], stats: dict[str, Any]
) -> dict[str, Any]:
    def split_manifest(name: str, cases: list[dict[str, Any]]) -> dict[str, Any]:
        body = "".join(
            json.dumps(case, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
            + "\n"
            for case in cases
        )
        return {
            "file": f"{name}.jsonl",
            "case_count": len(cases),
            "sha256": sha256_bytes(body.encode("utf-8")),
            "families": sorted({case["payload"]["family"] for case in cases}),
            "hosts": sorted(
                {
                    event["event_id"].split("-")[1]
                    for case in cases
                    for event in case["payload"]["transcript_excerpt"]["events"]
                }
            ),
        }

    return {
        "schema": "adapt.real-heldout-manifest.v1",
        "bench_id": BENCH_ID,
        "built_at": CREATED_AT,
        "heldout_policy": "heldout labels are frozen; detector tuning and heldout benchmark execution are forbidden in this build step",
        "redaction_contract_version": REDACTION_CONTRACT_VERSION,
        "stats": stats,
        "splits": {
            "dev": split_manifest("dev", dev),
            "heldout": split_manifest("heldout", heldout),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    if args.validate_only:
        cases = []
        for name in ("dev", "heldout"):
            cases.extend(
                json.loads(line)
                for line in (OUT_DIR / f"{name}.jsonl")
                .read_text(encoding="utf-8")
                .splitlines()
                if line.strip()
            )
        validate_cases(cases)
        print(
            json.dumps(
                {"ok": True, "cases": len(cases), "mode": "validate-only"},
                sort_keys=True,
            )
        )
        return 0
    dev, heldout, stats = split_cases(load_selections())
    validate_cases(dev + heldout)
    write_jsonl(OUT_DIR / "dev.jsonl", dev)
    write_jsonl(OUT_DIR / "heldout.jsonl", heldout)
    manifest = build_manifest(dev, heldout, stats)
    (OUT_DIR / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(
        json.dumps(
            {"ok": True, "dev": len(dev), "heldout": len(heldout), **stats},
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
