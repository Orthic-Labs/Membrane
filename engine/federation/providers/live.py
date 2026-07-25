"""Bounded working-tree overlay provider backed by the central freshness verdict."""
from __future__ import annotations

import hashlib
import os
import re
import subprocess
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any


DEFAULT_MAX_PATHS = 32
MAX_HISTORICAL_BLOB_BYTES = 1024 * 1024
MAX_OVERLAY_PATH_CHARS = 1024
_SAFE_RESOLVER_PATH = re.compile(r"[A-Za-z0-9._@+/-]+\Z")
_SAFE_SHA256 = re.compile(r"sha256:[0-9a-f]{64}\Z")


def _warning(kind: str, message: str) -> dict[str, str]:
    return {
        "provider": "live",
        "kind": kind,
        "severity": "warning",
        "message": message[:200],
    }


def _safe_relative_path(value: Any) -> str | None:
    raw = str(value or "").replace("\\", "/")
    if (
        not raw
        or len(raw) > MAX_OVERLAY_PATH_CHARS
        or raw != raw.strip()
        or raw.startswith("/")
        or any(ord(char) < 32 or ord(char) == 127 for char in raw)
    ):
        return None
    path = PurePosixPath(raw)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        return None
    normalized = path.as_posix()
    if normalized.startswith((".agent/", ".blueprint/")) or not _SAFE_RESOLVER_PATH.fullmatch(normalized):
        return None
    return normalized


def _sha256_bytes(body: bytes) -> str:
    return "sha256:" + hashlib.sha256(body).hexdigest()


def _file_sha256(path: Path) -> str | None:
    try:
        if path.is_symlink():
            return _sha256_bytes(os.readlink(path).encode("utf-8", errors="surrogatepass"))
        digest = hashlib.sha256()
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        return "sha256:" + digest.hexdigest()
    except OSError:
        return None


def _git_blob_sha256(
    repo_root: Path, base_commit: str, source_path: str
) -> tuple[str | None, str | None]:
    object_name = f"{base_commit}:{source_path}"
    try:
        size_result = subprocess.run(
            ["git", "-C", str(repo_root), "cat-file", "-s", object_name],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=5,
            check=False,
            text=True,
        )
        if size_result.returncode != 0:
            return None, None
        size = int(size_result.stdout.strip())
        if size < 0 or size > MAX_HISTORICAL_BLOB_BYTES:
            return None, "historical_blob_oversized"
        with tempfile.TemporaryFile() as body:
            result = subprocess.run(
                ["git", "-C", str(repo_root), "cat-file", "blob", object_name],
                stdout=body,
                stderr=subprocess.DEVNULL,
                timeout=5,
                check=False,
            )
            if result.returncode != 0 or body.tell() != size:
                return None, None
            body.seek(0)
            digest = hashlib.sha256()
            for chunk in iter(lambda: body.read(64 * 1024), b""):
                digest.update(chunk)
            return "sha256:" + digest.hexdigest(), None
    except (OSError, ValueError, subprocess.SubprocessError):
        return None, None


def _is_new_path(status: str) -> bool:
    return "?" in status or "A" in status


def _is_deleted(status: str) -> bool:
    return "D" in status


def _candidate_common(
    *,
    base_commit: str,
    overlay_digest: str,
    freshness_class: str,
) -> dict[str, str]:
    return {
        "baseCommit": base_commit,
        "overlayDigest": overlay_digest,
        "freshnessClass": freshness_class,
    }


def produce(
    repo_root: Path,
    *,
    base_commit: str | None = None,
    overlay_digest: str | None = None,
    overlay_entries: list[dict[str, Any]] | None = None,
    max_paths: int = DEFAULT_MAX_PATHS,
    prompt_fast: bool = False,
) -> tuple[list[dict[str, Any]], str, list[dict[str, str]]]:
    """Emit a verified committed baseline plus the bounded dirty overlay for each changed path.

    The central `/freshness` endpoint owns the Git/manifest verdict. This provider consumes its
    stable overlay entries. `prompt_fast` consumes the resident service's trusted snapshot hashes
    without repeating file/Git I/O; the candidate body is content-free path/status metadata, and
    its exact snapshot generation remains attached. Non-prompt callers retain immediate rechecks.
    """
    base = str(base_commit or "")
    digest = str(overlay_digest or "")
    entries = list(overlay_entries or [])
    cap = max(0, min(int(max_paths), 64))
    warnings: list[dict[str, str]] = []

    normalized_entries: list[tuple[str, dict[str, Any]]] = []
    for entry in entries:
        path = _safe_relative_path(entry.get("path"))
        if path is None:
            warnings.append(_warning("dirty_overlay_path_rejected", "unsafe overlay path omitted"))
            continue
        normalized_entries.append((path, entry))
    normalized_entries.sort(key=lambda item: item[0])
    if len(normalized_entries) > cap:
        warnings.append(
            _warning(
                "dirty_overlay_truncated",
                f"dirty overlay capped at {cap} of {len(normalized_entries)} paths",
            )
        )
    selected = normalized_entries[:cap]

    candidates: list[dict[str, Any]] = []
    for path, entry in selected:
        status = str(entry.get("status") or "??")[:2]
        old_path = _safe_relative_path(entry.get("oldPath")) if entry.get("oldPath") else None
        expected_hash = str(entry.get("contentHash") or "")
        absolute = repo_root / Path(path)

        if prompt_fast:
            if not _SAFE_SHA256.fullmatch(expected_hash):
                warnings.append(
                    _warning("dirty_overlay_hash_rejected", "invalid overlay hash omitted")
                )
                continue
            current_hash = expected_hash
        elif _is_deleted(status):
            if absolute.exists() or absolute.is_symlink():
                warnings.append(
                    _warning("dirty_overlay_changed", f"overlay changed after freshness verdict: {path}")
                )
                continue
            current_hash = expected_hash or _sha256_bytes(f"deleted:{path}".encode("utf-8"))
        else:
            current_hash = _file_sha256(absolute)
            if current_hash is None or current_hash != expected_hash:
                warnings.append(
                    _warning("dirty_overlay_changed", f"overlay changed after freshness verdict: {path}")
                )
                continue

        snapshot_path = old_path or path
        if not prompt_fast and base and not _is_new_path(status):
            committed_hash, historical_issue = _git_blob_sha256(repo_root, base, snapshot_path)
            if historical_issue:
                warnings.append(
                    _warning(historical_issue, f"historical snapshot omitted: {snapshot_path}")
                )
            if committed_hash is not None:
                candidates.append(
                    {
                        "id": f"git-snapshot:{base[:12]}:{snapshot_path}",
                        "layer": 3,
                        "sourceKind": "repo_code",
                        "sourceRef": f"{snapshot_path}@{base[:12]}",
                        "sourceHash": committed_hash,
                        "trustClass": "workspace_tracked",
                        "instructionPolicy": "data_only",
                        "providerScore": 0.45,
                        "scoreComponents": {"git_snapshot_relevance": 0.45, "freshness": 0.5},
                        "estimatedTokens": 24,
                        "protected": False,
                        "exact": True,
                        "recoverable": True,
                        "resolver": f"git show {base}:{snapshot_path}",
                        "text": f"committed snapshot path={snapshot_path}",
                        **_candidate_common(
                            base_commit=base,
                            overlay_digest=digest,
                            freshness_class="committed_snapshot",
                        ),
                    }
                )

        overlay_resolver = (
            f"git status --porcelain -- {path}"
            if _is_new_path(status)
            else f"git diff --no-ext-diff {base or 'HEAD'} -- {path}"
        )
        candidates.append(
            {
                "id": f"git-overlay:{path}",
                "layer": 3,
                "sourceKind": "repo_code_overlay",
                "sourceRef": path,
                "sourceHash": current_hash,
                "trustClass": "workspace_generated",
                "instructionPolicy": "data_only",
                "providerScore": 0.7,
                "scoreComponents": {"git_overlay_relevance": 0.7, "freshness": 1.0},
                "estimatedTokens": 32,
                "protected": False,
                "exact": True,
                "recoverable": True,
                "resolver": overlay_resolver,
                "text": f"working-tree status={status} path={path}",
                **_candidate_common(
                    base_commit=base,
                    overlay_digest=digest,
                    freshness_class="dirty_overlay",
                ),
            }
        )

    return candidates, digest or base or "dirty-overlay-unknown", warnings
