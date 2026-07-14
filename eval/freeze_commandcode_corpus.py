#!/usr/bin/env python3
"""Freeze the prompt corpus consumed by CommandCode's learn-taste pipeline."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import sys
import tempfile
from datetime import datetime
from pathlib import Path
from typing import NamedTuple, Sequence


SCHEMA_VERSION = "1.0.0"
DEFAULT_BATCH_BUDGET = 150_000
CAPTURE_BATCH_RE = re.compile(
    r"batch (?P<index>\d+)/(?P<total>\d+): "
    r"(?P<prompt_count>\d+) prompts sha256=(?P<sha256>[0-9a-f]{64}) "
    r"bytes=(?P<bytes>\d+)"
)


class PromptRecord(NamedTuple):
    source: str
    session_id: str
    ordinal: int
    text: str


class SessionRecord(NamedTuple):
    source: str
    session_id: str
    path: Path
    mtime_ns: int
    prompts: tuple[str, ...]


class ResolvedCorpus(NamedTuple):
    sessions: tuple[SessionRecord, ...]
    prompts: tuple[PromptRecord, ...]
    missing: dict[str, list[str]]


def estimate_text_tokens(text: str) -> int:
    if not text:
        return 0
    words = len(re.split(r"\s+", text))
    return max(0, math.ceil(len(text) / 3.5 - 0.1 * words))


def estimate_tokens(text: str) -> int:
    tokens = estimate_text_tokens(text)
    if text.startswith(("{", "[")):
        tokens = math.ceil(1.1 * tokens)
    return tokens


def _split_large_prompt(text: str, budget: int) -> list[str]:
    chunks: list[str] = []
    current = ""
    current_tokens = 0
    for line in text.split("\n"):
        line_tokens = estimate_tokens(line)
        if current_tokens + line_tokens <= budget:
            current += ("\n" if current else "") + line
            current_tokens += line_tokens
        else:
            if current:
                chunks.append(current)
            current = line
            current_tokens = line_tokens
    if current:
        chunks.append(current)
    return chunks


def split_prompts_into_batches(
    prompts: Sequence[str], *, budget: int = DEFAULT_BATCH_BUDGET
) -> list[list[str]]:
    if budget <= 0:
        raise ValueError("batch budget must be positive")
    batches: list[list[str]] = []
    current: list[str] = []
    current_tokens = 0
    for prompt in prompts:
        prompt_tokens = estimate_tokens(prompt)
        if prompt_tokens > budget:
            if current:
                batches.append(current)
                current = []
                current_tokens = 0
            batches.extend([[chunk] for chunk in _split_large_prompt(prompt, budget)])
        elif current_tokens + prompt_tokens <= budget:
            current.append(prompt)
            current_tokens += prompt_tokens
        else:
            if current:
                batches.append(current)
            current = [prompt]
            current_tokens = prompt_tokens
    if current:
        batches.append(current)
    return batches


def format_batch(prompts: Sequence[str]) -> str:
    return "\n\n".join(f"{index}. {prompt}" for index, prompt in enumerate(prompts, 1))


def _parse_timestamp(value: object) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def _within_cutoff(row: dict, cutoff: datetime | None) -> bool:
    if cutoff is None:
        return True
    timestamp = _parse_timestamp(row.get("timestamp"))
    return timestamp is None or timestamp <= cutoff


def _read_rows(path: Path):
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict):
                yield row


def extract_claude_prompts(path: Path, *, cutoff: datetime | None = None) -> list[str]:
    prompts: list[str] = []
    for row in _read_rows(path):
        message = row.get("message")
        if row.get("type") != "user" or not isinstance(message, dict):
            continue
        content = message.get("content")
        if _within_cutoff(row, cutoff) and isinstance(content, str) and content.strip():
            prompts.append(content.strip())
    return prompts


def extract_codex_prompts(path: Path, *, cutoff: datetime | None = None) -> list[str]:
    prompts: list[str] = []
    for row in _read_rows(path):
        payload = row.get("payload")
        if row.get("type") != "event_msg" or not isinstance(payload, dict):
            continue
        message = payload.get("message")
        if (
            payload.get("type") == "user_message"
            and _within_cutoff(row, cutoff)
            and isinstance(message, str)
            and message.strip()
        ):
            prompts.append(message.strip())
    return prompts


def read_codex_meta(path: Path) -> dict[str, str] | None:
    for row in _read_rows(path):
        payload = row.get("payload")
        if row.get("type") != "session_meta" or not isinstance(payload, dict):
            return None
        cwd = payload.get("cwd")
        session_id = payload.get("id") or payload.get("session_id")
        if not isinstance(cwd, str) or not isinstance(session_id, str):
            return None
        timestamp = payload.get("timestamp") or row.get("timestamp")
        result = {"id": session_id, "cwd": cwd}
        if isinstance(timestamp, str):
            result["timestamp"] = timestamp
        return result
    return None


def _codex_sort_mtime_ns(
    path: Path, *, current_mtime_ns: int, cutoff: datetime | None
) -> int:
    if cutoff is None or current_mtime_ns <= int(cutoff.timestamp() * 1_000_000_000):
        return current_mtime_ns
    latest: datetime | None = None
    for row in _read_rows(path):
        timestamp = _parse_timestamp(row.get("timestamp"))
        if timestamp is not None and timestamp <= cutoff:
            latest = timestamp if latest is None or timestamp > latest else latest
    return int(latest.timestamp() * 1_000_000_000) if latest else current_mtime_ns


def _learned_ids(settings_path: Path) -> dict[str, list[str]]:
    settings = json.loads(settings_path.read_text(encoding="utf-8"))
    learned = settings.get("tasteOnboarding", {}).get("learnedSessions", {})
    return {
        "claude-code": list(learned.get("claude-code", [])),
        "codex": list(learned.get("codex", [])),
    }


def _codex_files(roots: Sequence[Path], wanted_ids: set[str]) -> list[Path]:
    files: list[Path] = []
    for root in roots:
        if root.is_dir():
            files.extend(
                path
                for path in root.rglob("rollout-*.jsonl")
                if path.is_file() and any(session_id in path.name for session_id in wanted_ids)
            )
    return files


def resolve_corpus(
    *,
    settings_path: Path,
    claude_dir: Path,
    codex_roots: Sequence[Path],
    cwd: str,
    cutoff: datetime | None = None,
) -> ResolvedCorpus:
    learned = _learned_ids(settings_path)
    claude_wanted = set(learned["claude-code"])
    claude_sessions: list[SessionRecord] = []
    found_claude: set[str] = set()
    if claude_dir.is_dir():
        with os.scandir(claude_dir) as entries:
            for entry in entries:
                if not entry.is_file() or not entry.name.endswith(".jsonl"):
                    continue
                session_id = entry.name[:-6]
                if session_id not in claude_wanted or ".checkpoints" in entry.name:
                    continue
                path = Path(entry.path)
                prompts = tuple(extract_claude_prompts(path, cutoff=cutoff))
                stat = entry.stat()
                claude_sessions.append(SessionRecord(
                    "claude-code", session_id, path, stat.st_mtime_ns, prompts
                ))
                found_claude.add(session_id)

    codex_wanted = set(learned["codex"])
    codex_by_id: dict[str, SessionRecord] = {}
    for path in _codex_files(codex_roots, codex_wanted):
        meta = read_codex_meta(path)
        if not meta or meta["id"] not in codex_wanted or meta["cwd"] != cwd:
            continue
        stat = path.stat()
        record = SessionRecord(
            "codex",
            meta["id"],
            path,
            _codex_sort_mtime_ns(
                path, current_mtime_ns=stat.st_mtime_ns, cutoff=cutoff
            ),
            tuple(extract_codex_prompts(path, cutoff=cutoff)),
        )
        previous = codex_by_id.get(record.session_id)
        if previous is None or record.mtime_ns > previous.mtime_ns:
            codex_by_id[record.session_id] = record
    codex_sessions = sorted(codex_by_id.values(), key=lambda item: item.mtime_ns, reverse=True)

    groups = [claude_sessions, codex_sessions]
    groups = [group for group in groups if group]
    groups.sort(key=len, reverse=True)
    sessions = tuple(session for group in groups for session in group)
    prompts = tuple(
        PromptRecord(session.source, session.session_id, ordinal, text)
        for session in sessions
        for ordinal, text in enumerate(session.prompts, 1)
    )
    missing = {
        "claude-code": [
            session_id for session_id in learned["claude-code"]
            if session_id not in found_claude
        ],
        "codex": [
            session_id for session_id in learned["codex"]
            if session_id not in codex_by_id
        ],
    }
    return ResolvedCorpus(sessions, prompts, missing)


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _safe_path(root: Path, relative: str) -> Path:
    path = (root / relative).resolve()
    if root.resolve() not in path.parents:
        raise ValueError(f"manifest path escapes corpus root: {relative}")
    return path


def freeze_corpus(
    *,
    settings_path: Path,
    claude_dir: Path,
    codex_roots: Sequence[Path],
    cwd: str,
    output_dir: Path,
    budget: int = DEFAULT_BATCH_BUDGET,
    cutoff: datetime | None = None,
) -> dict:
    if output_dir.exists():
        raise FileExistsError(f"output directory already exists: {output_dir}")
    corpus = resolve_corpus(
        settings_path=settings_path,
        claude_dir=claude_dir,
        codex_roots=codex_roots,
        cwd=cwd,
        cutoff=cutoff,
    )
    prompt_batches = split_prompts_into_batches(
        [record.text for record in corpus.prompts], budget=budget
    )
    payloads = [format_batch(batch) for batch in prompt_batches]
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(
        prefix=f".{output_dir.name}.tmp-", dir=output_dir.parent
    ))
    try:
        batches_dir = staging / "batches"
        batches_dir.mkdir()

        prompt_lines = [json.dumps({
            "source": record.source,
            "session_id": record.session_id,
            "ordinal": record.ordinal,
            "text": record.text,
        }, ensure_ascii=False, sort_keys=True) for record in corpus.prompts]
        prompt_bytes = (("\n".join(prompt_lines) + "\n") if prompt_lines else "").encode("utf-8")
        (staging / "prompts.jsonl").write_bytes(prompt_bytes)

        batches: list[dict] = []
        for index, payload in enumerate(payloads, 1):
            relative = f"batches/batch-{index:03d}.txt"
            data = payload.encode("utf-8")
            (staging / relative).write_bytes(data)
            batches.append({
                "index": index,
                "path": relative,
                "prompt_count": len(prompt_batches[index - 1]),
                "bytes": len(data),
                "sha256": _sha256(data),
            })

        manifest = {
            "schema_version": SCHEMA_VERSION,
            "cwd": cwd,
            "cutoff": cutoff.isoformat() if cutoff else None,
            "batch_budget": budget,
            "session_count": len(corpus.sessions),
            "prompt_count": len(corpus.prompts),
            "batch_count": len(batches),
            "missing": corpus.missing,
            "privacy": {"raw_local_only": True, "provider_calls": 0, "memright_writes": 0},
            "prompts": {"path": "prompts.jsonl", "bytes": len(prompt_bytes), "sha256": _sha256(prompt_bytes)},
            "sessions": [{
                "source": session.source,
                "session_id": session.session_id,
                "path": session.path.as_posix(),
                "mtime_ns": session.mtime_ns,
                "prompt_count": len(session.prompts),
            } for session in corpus.sessions],
            "batches": batches,
        }
        content = json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        (staging / "manifest.json").write_text(content, encoding="utf-8")
        validate_manifest(staging / "manifest.json")
        staging.replace(output_dir)
        return manifest
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def validate_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    root = path.parent
    prompts = manifest["prompts"]
    prompt_data = _safe_path(root, prompts["path"]).read_bytes()
    if len(prompt_data) != prompts["bytes"] or _sha256(prompt_data) != prompts["sha256"]:
        raise ValueError("frozen prompt corpus hash mismatch")
    for batch in manifest["batches"]:
        data = _safe_path(root, batch["path"]).read_bytes()
        if len(data) != batch["bytes"] or _sha256(data) != batch["sha256"]:
            raise ValueError(f"frozen batch hash mismatch: {batch['index']}")
    if len(manifest["batches"]) != manifest["batch_count"]:
        raise ValueError("frozen batch count mismatch")
    return manifest


def parse_capture_log(path: Path) -> list[dict]:
    captured: list[dict] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        match = CAPTURE_BATCH_RE.search(line)
        if not match:
            continue
        captured.append({
            "index": int(match.group("index")),
            "total": int(match.group("total")),
            "prompt_count": int(match.group("prompt_count")),
            "sha256": match.group("sha256"),
            "bytes": int(match.group("bytes")),
        })
    return captured


def compare_batches(manifest: dict, captured: Sequence[dict]) -> dict:
    actual_by_index = {item["index"]: item for item in manifest["batches"]}
    exact: list[int] = []
    mismatches: list[dict] = []
    for expected in captured:
        actual = actual_by_index.get(expected["index"])
        if actual is None:
            mismatches.append({"index": expected["index"], "fields": ["missing"]})
            continue
        fields = [
            field for field in ("prompt_count", "bytes", "sha256")
            if actual[field] != expected[field]
        ]
        if fields:
            mismatches.append({"index": expected["index"], "fields": fields})
        else:
            exact.append(expected["index"])
    return {
        "captured_count": len(captured),
        "exact_count": len(exact),
        "exact_batches": exact,
        "mismatches": mismatches,
    }


def _args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Freeze a CommandCode-compatible learn-taste prompt corpus."
    )
    parser.add_argument("--settings", type=Path, required=True)
    parser.add_argument("--claude-dir", type=Path, required=True)
    parser.add_argument("--codex-root", type=Path, action="append", default=[])
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cutoff", help="ISO-8601 historical snapshot cutoff")
    parser.add_argument("--budget", type=int, default=DEFAULT_BATCH_BUDGET)
    parser.add_argument("--capture-log", type=Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _args(argv)
    cutoff = _parse_timestamp(args.cutoff) if args.cutoff else None
    if args.cutoff and cutoff is None:
        raise ValueError(f"invalid cutoff timestamp: {args.cutoff}")
    manifest = freeze_corpus(
        settings_path=args.settings,
        claude_dir=args.claude_dir,
        codex_roots=args.codex_root,
        cwd=args.cwd,
        output_dir=args.output,
        budget=args.budget,
        cutoff=cutoff,
    )
    summary = {
        "batch_count": manifest["batch_count"],
        "missing": manifest["missing"],
        "prompt_count": manifest["prompt_count"],
        "session_count": manifest["session_count"],
    }
    if args.capture_log:
        summary["capture_comparison"] = compare_batches(
            manifest, parse_capture_log(args.capture_log)
        )
    print(json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
