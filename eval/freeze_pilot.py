"""Freeze a deterministic, local-only Adapt pilot corpus without model calls.

The package contains selected raw transcripts, parsed/redacted corpus rows,
exact extraction payloads, authority source copies, recursive initial-rule
artifacts, and a strict content-hashed manifest. Nothing is sent externally and
no Crypt service is contacted.
"""
from __future__ import annotations

import argparse
import collections
import dataclasses
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Iterable

ADAPT_DIR = Path(__file__).resolve().parent.parent
SCHEMA_PATH = Path(__file__).resolve().parent / "pilot-corpus.schema.json"
sys.path.insert(0, str(ADAPT_DIR))

import workspace_runtime  # noqa: E402

REPO_ROOT = workspace_runtime.workspace_root()

import adapt_llm  # noqa: E402
import adapt_sessions  # noqa: E402
import authority  # noqa: E402

SCHEMA_VERSION = "1.0.0"
SELECTION_STRATEGY = "balanced-tool-round-robin(scope,recency,size)-v1"
SIZE_BUCKETS = ((2_000, "small"), (12_000, "medium"))
RECENCY_BUCKETS = ("recent", "middle", "older")
DEFAULT_TARGET = 50
DEFAULT_MAX_RAW_BYTES = 1_000_000_000
DEFAULT_SETTLE_SECONDS = 300
_SAFE_RE = re.compile(r"[^a-zA-Z0-9._-]+")


def _json_bytes(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _portable_path(path: Path) -> str:
    resolved = path.resolve()
    try:
        return "~/" + resolved.relative_to(Path.home().resolve()).as_posix()
    except ValueError:
        return resolved.as_posix()


def _safe(value: str) -> str:
    return _SAFE_RE.sub("-", value).strip("-") or "item"


def _identity(session: adapt_sessions.Session) -> tuple[str, str, str]:
    return (session.tool, session.session_id, session.path.resolve().as_posix())


def primary_scope(session: adapt_sessions.Session) -> str:
    counts = collections.Counter(turn.scope for turn in session.turns)
    if not counts:
        return adapt_sessions.scope_for_cwd(session.cwd)
    maximum = max(counts.values())
    return min(scope for scope, count in counts.items() if count == maximum)


def _char_count(session: adapt_sessions.Session) -> int:
    return sum(len(turn.text) for turn in session.turns)


def _size_bucket(session: adapt_sessions.Session) -> str:
    count = _char_count(session)
    for ceiling, label in SIZE_BUCKETS:
        if count <= ceiling:
            return label
    return "large"


def _recency_map(sessions: Iterable[adapt_sessions.Session]) -> dict[tuple[str, str, str], str]:
    by_tool: dict[str, list[adapt_sessions.Session]] = collections.defaultdict(list)
    for session in sessions:
        by_tool[session.tool].append(session)
    result: dict[tuple[str, str, str], str] = {}
    for tool_sessions in by_tool.values():
        ordered = sorted(
            tool_sessions,
            key=lambda item: (-item.mtime, item.session_id, item.path.resolve().as_posix()),
        )
        total = len(ordered)
        for index, session in enumerate(ordered):
            bucket_index = min(2, (index * 3) // max(1, total))
            result[_identity(session)] = RECENCY_BUCKETS[bucket_index]
    return result


def _stratum_key(
    session: adapt_sessions.Session,
    recency: dict[tuple[str, str, str], str],
) -> tuple[str, str, str]:
    return (primary_scope(session), recency[_identity(session)], _size_bucket(session))


def select_sessions(
    sessions: Iterable[adapt_sessions.Session],
    *,
    target: int = DEFAULT_TARGET,
) -> list[adapt_sessions.Session]:
    """Balance tools, then round-robin scope/recency/size strata deterministically."""
    if target <= 0:
        return []
    unique = {_identity(session): session for session in sessions if session.turns}
    ordered = [unique[key] for key in sorted(unique)]
    recency = _recency_map(ordered)
    grouped: dict[str, dict[tuple[str, str, str], list[adapt_sessions.Session]]] = (
        collections.defaultdict(lambda: collections.defaultdict(list))
    )
    for session in ordered:
        grouped[session.tool][_stratum_key(session, recency)].append(session)
    for strata in grouped.values():
        for queue in strata.values():
            queue.sort(
                key=lambda item: (-item.mtime, item.session_id, item.path.resolve().as_posix())
            )

    stratum_order = {tool: sorted(strata) for tool, strata in grouped.items()}
    pointers = {tool: 0 for tool in grouped}
    selected: list[adapt_sessions.Session] = []
    tools = sorted(grouped)
    while len(selected) < min(target, len(ordered)):
        progressed = False
        for tool in tools:
            keys = stratum_order[tool]
            if not keys:
                continue
            for _ in range(len(keys)):
                index = pointers[tool] % len(keys)
                pointers[tool] += 1
                queue = grouped[tool][keys[index]]
                if queue:
                    selected.append(queue.pop(0))
                    progressed = True
                    break
            if len(selected) >= min(target, len(ordered)):
                break
        if not progressed:
            break
    return selected


def filter_settled_sessions(
    sessions: Iterable[adapt_sessions.Session],
    *,
    eligible_before_ns: int | None,
) -> list[adapt_sessions.Session]:
    if eligible_before_ns is None:
        return list(sessions)
    return [
        session for session in sessions
        if session.path.stat().st_mtime_ns <= eligible_before_ns
    ]


def manifest_sha256(manifest: dict) -> str:
    payload = {key: value for key, value in manifest.items() if key != "manifest_sha256"}
    return _sha256_bytes(_json_bytes(payload))


def _content_sha256(manifest: dict) -> str:
    excluded = {"content_sha256", "pilot_id", "manifest_sha256"}
    payload = {key: value for key, value in manifest.items() if key not in excluded}
    return _sha256_bytes(_json_bytes(payload))


def _copy_verified(source: Path, destination: Path) -> tuple[int, str]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    source_hash = _sha256_file(source)
    if _sha256_file(destination) != source_hash:
        raise OSError(f"copy hash mismatch: {source}")
    return source.stat().st_size, source_hash


def _artifact_files(roots: Iterable[Path]) -> list[tuple[str, Path, Path, str]]:
    result: list[tuple[str, Path, Path, str]] = []
    seen: set[Path] = set()
    for index, candidate in enumerate(roots, 1):
        root = candidate.resolve()
        if not root.exists() or root in seen:
            continue
        seen.add(root)
        root_id = f"root-{index:02d}-{_safe(root.name)}"
        files = [root] if root.is_file() else sorted(path for path in root.rglob("*") if path.is_file())
        for path in files:
            if path.is_symlink():
                raise ValueError(f"artifact symlink refused: {path}")
            relative = path.name if root.is_file() else path.relative_to(root).as_posix()
            result.append((root_id, root, path, relative))
    return result


def _turns_sha256(session: adapt_sessions.Session) -> str:
    return _sha256_bytes(_json_bytes([
        {"scope": turn.scope, "text": turn.text}
        for turn in session.turns
    ]))


def _stable_source_hash(session: adapt_sessions.Session) -> str:
    before = _sha256_file(session.path)
    parser = (
        adapt_sessions.parse_claude_session
        if session.tool == "claude-code"
        else adapt_sessions.parse_codex_session
    )
    reparsed = parser(session.path)
    after = _sha256_file(session.path)
    if before != after or reparsed is None or _turns_sha256(reparsed) != _turns_sha256(session):
        raise ValueError(f"session changed after discovery: {session.path}")
    return before


def _git_value(args: list[str]) -> str:
    try:
        completed = subprocess.run(
            ["git", "-C", str(REPO_ROOT), *args],
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return "unavailable"
    return completed.stdout.strip() if completed.returncode == 0 else "unavailable"


def _version_record(out_dir: Path) -> dict:
    files = [
        ADAPT_DIR / "adapt_sessions.py",
        ADAPT_DIR / "adapt_llm.py",
        ADAPT_DIR / "admission.py",
        ADAPT_DIR / "outcomes.py",
        Path(__file__).resolve(),
        SCHEMA_PATH,
    ]
    records = []
    for path in files:
        source_rel = path.relative_to(REPO_ROOT)
        copy_rel = Path("tool-sources") / source_rel
        _, digest = _copy_verified(path, out_dir / copy_rel)
        records.append({
            "path": source_rel.as_posix(),
            "copy": copy_rel.as_posix(),
            "sha256": digest,
        })
    return {
        "git_head": _git_value(["rev-parse", "HEAD"]),
        "git_dirty": bool(_git_value(["status", "--porcelain"])),
        "files": records,
        "constants": {
            "batch_char_budget": adapt_llm.BATCH_CHAR_BUDGET,
            "min_turn_chars": adapt_sessions.MIN_TURN_CHARS,
            "max_turn_chars": adapt_sessions.MAX_TURN_CHARS,
            "max_turns_per_session": adapt_sessions.MAX_TURNS_PER_SESSION,
            "extract_system_sha256": _sha256_bytes(adapt_llm.EXTRACT_SYSTEM.encode("utf-8")),
            "synth_system_sha256": _sha256_bytes(adapt_llm.SYNTH_SYSTEM.encode("utf-8")),
        },
    }


def freeze_pilot(
    sessions: Iterable[adapt_sessions.Session],
    out_dir: Path,
    *,
    target: int = DEFAULT_TARGET,
    workspace_root: Path = REPO_ROOT,
    artifact_roots: Iterable[Path] = (),
    max_raw_bytes: int = DEFAULT_MAX_RAW_BYTES,
    eligible_before_ns: int | None = None,
) -> dict:
    """Write a frozen local evidence package and return its manifest."""
    all_sessions = list(sessions)
    eligible_sessions = filter_settled_sessions(
        all_sessions, eligible_before_ns=eligible_before_ns
    )
    selected = select_sessions(eligible_sessions, target=target)
    raw_bytes = sum(session.path.stat().st_size for session in selected)
    if raw_bytes > max_raw_bytes:
        raise ValueError(
            f"raw transcript byte ceiling exceeded: {raw_bytes} > {max_raw_bytes}"
        )
    stable_hashes = {
        _identity(session): _stable_source_hash(session)
        for session in selected
    }
    if out_dir.exists():
        raise FileExistsError(f"output already exists: {out_dir}")
    out_dir.mkdir(parents=True)

    recency = _recency_map(eligible_sessions)
    corpus_rows: list[dict] = []
    session_records: list[dict] = []
    for session_index, session in enumerate(selected, 1):
        raw_rel = (
            Path("raw")
            / f"{session_index:03d}-{_safe(session.tool)}-{_safe(session.session_id)}.jsonl"
        )
        source_size, source_hash = _copy_verified(session.path, out_dir / raw_rel)
        if source_hash != stable_hashes[_identity(session)]:
            raise ValueError(f"session changed while freezing: {session.path}")
        turn_payload = [
            {"scope": turn.scope, "text": turn.text}
            for turn in session.turns
        ]
        for turn_index, turn in enumerate(session.turns, 1):
            corpus_rows.append({
                "ordinal": len(corpus_rows) + 1,
                "session_ordinal": session_index,
                "turn_ordinal": turn_index,
                "session_id": session.session_id,
                "tool": session.tool,
                "scope": turn.scope,
                "text": turn.text,
            })
        stats = dataclasses.asdict(session.stats)
        session_records.append({
            "ordinal": session_index,
            "tool": session.tool,
            "session_id": session.session_id,
            "source_path": _portable_path(session.path),
            "source_sha256": source_hash,
            "source_bytes": source_size,
            "raw_copy": raw_rel.as_posix(),
            "mtime_ns": session.path.stat().st_mtime_ns,
            "primary_scope": primary_scope(session),
            "recency_bucket": recency[_identity(session)],
            "size_bucket": _size_bucket(session),
            "turn_count": len(session.turns),
            "char_count": _char_count(session),
            "turns_sha256": _sha256_bytes(_json_bytes(turn_payload)),
            "stats": stats,
        })

    corpus_bytes = b"".join(_json_bytes(row) + b"\n" for row in corpus_rows)
    corpus_rel = Path("corpus.jsonl")
    (out_dir / corpus_rel).write_bytes(corpus_bytes)
    turns = [(row["tool"], row["scope"], row["text"]) for row in corpus_rows]
    batches = []
    for index, batch in enumerate(adapt_llm.build_batches(turns), 1):
        payload = adapt_llm.build_extract_payload(batch).encode("utf-8")
        payload_rel = Path("extraction-payloads") / f"batch-{index:03d}.json"
        (out_dir / payload_rel).parent.mkdir(parents=True, exist_ok=True)
        (out_dir / payload_rel).write_bytes(payload)
        batches.append({
            "index": index,
            "path": payload_rel.as_posix(),
            "turn_count": len(batch),
            "char_count": sum(len(turn[2]) for turn in batch),
            "payload_sha256": _sha256_bytes(payload),
        })

    authority_manifest = authority.build_manifest(workspace_root)
    authority_copies = []
    for source in authority_manifest["sources"]:
        source_path = workspace_root.resolve() / source["path"]
        copy_rel = Path("authority-sources") / source["path"]
        size, digest = _copy_verified(source_path, out_dir / copy_rel)
        if digest != source["sha256"]:
            raise ValueError(f"authority source changed while freezing: {source_path}")
        authority_copies.append({
            "source_path": source["path"],
            "copy": copy_rel.as_posix(),
            "bytes": size,
            "sha256": digest,
        })

    artifact_records = []
    for root_id, source_root, source, relative in _artifact_files(artifact_roots):
        copy_rel = Path("initial-artifacts") / root_id / relative
        size, digest = _copy_verified(source, out_dir / copy_rel)
        artifact_records.append({
            "root_id": root_id,
            "source_root": _portable_path(source_root),
            "path": relative,
            "copy": copy_rel.as_posix(),
            "bytes": size,
            "sha256": digest,
        })
    artifact_records.sort(key=lambda item: (item["root_id"], item["path"]))
    artifact_tree_hash = _sha256_bytes(_json_bytes(artifact_records))

    stratum_counts = collections.Counter(
        f"{item['tool']}|{item['primary_scope']}|{item['recency_bucket']}|{item['size_bucket']}"
        for item in session_records
    )
    quality = _aggregate_stats(
        session_records, [field.name for field in dataclasses.fields(adapt_sessions.ParseStats)]
    )
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "privacy": {
            "raw_local_only": True,
            "external_review_safe": False,
            "contains_raw_transcripts": True,
        },
        "selection": {
            "strategy": SELECTION_STRATEGY,
            "target_sessions": target,
            "discovered_sessions": len(all_sessions),
            "available_sessions": len(eligible_sessions),
            "selected_sessions": len(selected),
            "shortfall": max(0, target - len(selected)),
            "eligible_before_ns": eligible_before_ns,
            "raw_bytes": raw_bytes,
            "max_raw_bytes": max_raw_bytes,
            "strata_counts": dict(sorted(stratum_counts.items())),
        },
        "corpus": {
            "path": corpus_rel.as_posix(),
            "sha256": _sha256_bytes(corpus_bytes),
            "session_count": len(selected),
            "turn_count": len(corpus_rows),
            "char_count": sum(len(row["text"]) for row in corpus_rows),
            "order_sha256": _sha256_bytes(
                _json_bytes([
                    [row["session_id"], row["turn_ordinal"], row["tool"], row["scope"]]
                    for row in corpus_rows
                ])
            ),
        },
        "quality": quality,
        "sessions": session_records,
        "extraction": {
            "batch_char_budget": adapt_llm.BATCH_CHAR_BUDGET,
            "batch_count": len(batches),
            "batches": batches,
        },
        "versions": _version_record(out_dir),
        "authority": {
            "manifest": authority_manifest,
            "copies": authority_copies,
        },
        "initial_artifacts": {
            "files": artifact_records,
            "tree_sha256": artifact_tree_hash,
        },
    }
    content_hash = _content_sha256(manifest)
    manifest["content_sha256"] = content_hash
    manifest["pilot_id"] = f"adapt-pilot-{content_hash[:12]}"
    manifest["manifest_sha256"] = manifest_sha256(manifest)
    manifest_path = out_dir / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8"
    )
    validate_manifest(manifest_path)
    return manifest


def _safe_package_path(root: Path, relative: str) -> Path:
    path = (root / relative).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError as exc:
        raise ValueError(f"package path escapes root: {relative}") from exc
    return path


def _aggregate_stats(records: list[dict], keys) -> dict:
    totals = {}
    for key in keys:
        values = [item["stats"][key] for item in records]
        if values and isinstance(values[0], dict):
            totals[key] = dict(sum((collections.Counter(value) for value in values),
                                   collections.Counter()))
        else:
            totals[key] = sum(values)
    return totals


def validate_manifest(path: Path) -> dict:
    raw = json.loads(path.read_text(encoding="utf-8"))
    try:
        import jsonschema
        jsonschema.validate(raw, json.loads(SCHEMA_PATH.read_text(encoding="utf-8")))
    except ImportError as exc:
        raise ValueError("jsonschema is required to validate pilot manifests") from exc
    except jsonschema.ValidationError as exc:
        raise ValueError(f"pilot manifest schema invalid: {exc.message}") from exc
    if raw["manifest_sha256"] != manifest_sha256(raw):
        raise ValueError("pilot manifest hash mismatch")
    if raw["content_sha256"] != _content_sha256(raw):
        raise ValueError("pilot content hash mismatch")
    if raw["pilot_id"] != f"adapt-pilot-{raw['content_sha256'][:12]}":
        raise ValueError("pilot id does not match content hash")
    package_root = path.parent
    checks = [(raw["corpus"]["path"], raw["corpus"]["sha256"])]
    checks.extend((item["raw_copy"], item["source_sha256"]) for item in raw["sessions"])
    checks.extend((item["path"], item["payload_sha256"]) for item in raw["extraction"]["batches"])
    checks.extend((item["copy"], item["sha256"]) for item in raw["versions"]["files"])
    checks.extend((item["copy"], item["sha256"]) for item in raw["authority"]["copies"])
    checks.extend((item["copy"], item["sha256"]) for item in raw["initial_artifacts"]["files"])
    failures = []
    for relative, expected in checks:
        target = _safe_package_path(package_root, relative)
        if not target.is_file() or _sha256_file(target) != expected:
            failures.append(relative)
    if failures:
        raise ValueError(f"pilot package file hash mismatch: {failures}")
    if raw["initial_artifacts"]["tree_sha256"] != _sha256_bytes(
        _json_bytes(raw["initial_artifacts"]["files"])
    ):
        raise ValueError("initial artifact tree hash mismatch")
    authority_hashes = {
        item["path"]: item["sha256"]
        for item in raw["authority"]["manifest"]["sources"]
    }
    if any(
        authority_hashes.get(item["source_path"]) != item["sha256"]
        for item in raw["authority"]["copies"]
    ):
        raise ValueError("authority copies do not match authority manifest")
    if raw["corpus"]["session_count"] != len(raw["sessions"]):
        raise ValueError("corpus session count mismatch")
    quality = _aggregate_stats(raw["sessions"], raw["quality"])
    if raw["quality"] != quality:
        raise ValueError("corpus quality totals mismatch")
    if raw["quality"]["kept_turns"] != raw["corpus"]["turn_count"]:
        raise ValueError("kept turn count does not match corpus")
    if raw["selection"]["selected_sessions"] != len(raw["sessions"]):
        raise ValueError("selected session count mismatch")
    if raw["selection"]["raw_bytes"] != sum(
        item["source_bytes"] for item in raw["sessions"]
    ):
        raise ValueError("raw byte count mismatch")
    if raw["extraction"]["batch_count"] != len(raw["extraction"]["batches"]):
        raise ValueError("extraction batch count mismatch")
    if raw["corpus"]["turn_count"] != sum(
        item["turn_count"] for item in raw["extraction"]["batches"]
    ):
        raise ValueError("extraction turn count mismatch")
    return raw


def discover_all_sessions() -> list[adapt_sessions.Session]:
    sessions = []
    for tool, path in adapt_sessions.discover():
        parser = (
            adapt_sessions.parse_claude_session
            if tool == "claude-code"
            else adapt_sessions.parse_codex_session
        )
        session = parser(path)
        if session is not None and not adapt_sessions.scope_excluded(session.cwd):
            sessions.append(session)
    return sessions


def _default_artifact_roots(workspace_root: Path) -> list[Path]:
    candidates = [
        workspace_root / ".commandcode" / "taste",
        Path.home() / ".commandcode" / "taste",
        adapt_sessions.STATE_DIR / "rules.json",
        adapt_sessions.STATE_DIR / "adapt-digest.md",
    ]
    unique = []
    seen = set()
    for path in candidates:
        resolved = path.resolve()
        if resolved.exists() and resolved not in seen:
            unique.append(resolved)
            seen.add(resolved)
    return unique


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--target", type=int, default=DEFAULT_TARGET)
    parser.add_argument("--workspace-root", type=Path, default=REPO_ROOT)
    parser.add_argument("--artifact-root", type=Path, action="append", default=None)
    parser.add_argument("--max-raw-bytes", type=int, default=DEFAULT_MAX_RAW_BYTES)
    cutoff = parser.add_mutually_exclusive_group()
    cutoff.add_argument("--settle-seconds", type=int)
    cutoff.add_argument("--eligible-before-ns", type=int)
    parser.add_argument("--validate-only", type=Path, default=None)
    args = parser.parse_args(argv)
    if args.validate_only:
        manifest = validate_manifest(args.validate_only)
        print(
            f"valid pilot={manifest['pilot_id']} sessions="
            f"{manifest['selection']['selected_sessions']} "
            f"batches={manifest['extraction']['batch_count']}"
        )
        return 0
    if args.out is None:
        parser.error("--out is required unless --validate-only is used")
    settle_seconds = (
        DEFAULT_SETTLE_SECONDS
        if args.settle_seconds is None
        else args.settle_seconds
    )
    sessions = discover_all_sessions()
    roots = args.artifact_root or _default_artifact_roots(args.workspace_root)
    if (
        args.target <= 0
        or args.max_raw_bytes <= 0
        or settle_seconds < 0
        or (args.eligible_before_ns is not None and args.eligible_before_ns < 0)
    ):
        parser.error("target/max-raw-bytes must be positive and settle-seconds non-negative")
    eligible_before_ns = args.eligible_before_ns
    if eligible_before_ns is None:
        eligible_before_ns = time.time_ns() - settle_seconds * 1_000_000_000
    manifest = freeze_pilot(
        sessions,
        args.out,
        target=args.target,
        workspace_root=args.workspace_root,
        artifact_roots=roots,
        max_raw_bytes=args.max_raw_bytes,
        eligible_before_ns=eligible_before_ns,
    )
    print(
        f"frozen pilot={manifest['pilot_id']} available="
        f"{manifest['selection']['available_sessions']} selected="
        f"{manifest['selection']['selected_sessions']} shortfall="
        f"{manifest['selection']['shortfall']} turns={manifest['corpus']['turn_count']} "
        f"batches={manifest['extraction']['batch_count']} raw_bytes="
        f"{manifest['selection']['raw_bytes']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
