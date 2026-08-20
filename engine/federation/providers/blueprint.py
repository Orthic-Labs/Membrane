"""Blueprint provider over resident daemon IPC."""
from __future__ import annotations

import hashlib
import json
import os
import socket
import sys
import time
import uuid
from pathlib import Path
from typing import Any

BLUEPRINT_CANDIDATE_CAP_DEFAULT = 64
BLUEPRINT_TIMEOUT_S = 1.2
BLUEPRINT_CACHE_TTL_S = 300.0
BLUEPRINT_CACHE_MAX_ENTRIES = 16

_candidate_cache: dict[tuple[str, str, str, int], tuple[float, list[dict]]] = {}


def candidate_cap(max_tokens: int, raw_override: str | None = None) -> int:
    raw = raw_override if raw_override is not None else os.environ.get("MEMBRANE_BLUEPRINT_CAP")
    try:
        configured = int(raw) if raw is not None else BLUEPRINT_CANDIDATE_CAP_DEFAULT
    except ValueError:
        configured = BLUEPRINT_CANDIDATE_CAP_DEFAULT
    return min(max(1, configured), 256, max(1, max_tokens))


def _cache_key(repo_root: Path, generation: str, task: str, cap: int) -> tuple[str, str, str, int]:
    return (str(repo_root.resolve()), generation, hashlib.sha256(task.encode()).hexdigest(), cap)


def _cached_candidates(key: tuple[str, str, str, int]) -> list[dict] | None:
    cached = _candidate_cache.get(key)
    if cached is None or cached[0] <= time.monotonic():
        _candidate_cache.pop(key, None)
        return None
    return [dict(candidate) for candidate in cached[1]]


def _cache_candidates(key: tuple[str, str, str, int], candidates: list[dict]) -> None:
    if len(_candidate_cache) >= BLUEPRINT_CACHE_MAX_ENTRIES:
        oldest = min(_candidate_cache, key=lambda entry: _candidate_cache[entry][0])
        _candidate_cache.pop(oldest, None)
    _candidate_cache[key] = (time.monotonic() + BLUEPRINT_CACHE_TTL_S, [dict(candidate) for candidate in candidates])


def _daemon_endpoint() -> str:
    explicit = os.environ.get("BLUEPRINT_DAEMON_ENDPOINT")
    if explicit:
        return explicit
    if sys.platform == "win32":
        suffix = hashlib.sha256(str(Path.home()).encode()).hexdigest()[:16]
        return rf"\\.\pipe\orthic-blueprint-{suffix}"
    return str(Path.home() / ".blueprint" / "blueprint.sock")


def _read_daemon_request(
    repo_root: Path,
    method: str,
    input_payload: dict[str, Any],
    generation: str | None,
    *,
    deadline_s: float = BLUEPRINT_TIMEOUT_S,
) -> dict[str, Any]:
    request_id = str(uuid.uuid4())
    request = {
        "protocolVersion": 1,
        "requestId": request_id,
        "repoId": None,
        "generation": generation,
        "method": method,
        "deadlineMs": max(10, int(deadline_s * 1000)),
        "input": {"repoRoot": str(repo_root.resolve()), **input_payload},
    }
    wire = (json.dumps(request, separators=(",", ":")) + "\n").encode()
    endpoint = _daemon_endpoint()
    if sys.platform == "win32":
        with open(endpoint, "r+b", buffering=0) as pipe:
            pipe.write(wire)
            line = pipe.readline()
    else:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(deadline_s)
            client.connect(endpoint)
            client.sendall(wire)
            chunks = bytearray()
            while b"\n" not in chunks:
                block = client.recv(65536)
                if not block:
                    break
                chunks.extend(block)
            line = bytes(chunks).split(b"\n", 1)[0]
    if not line:
        raise ConnectionError("Blueprint daemon returned no response")
    response = json.loads(line)
    if response.get("requestId") != request_id:
        raise RuntimeError("Blueprint daemon response identity mismatch")
    if not response.get("ok"):
        error = response.get("error") or {}
        raise RuntimeError(f"{error.get('code', 'daemon_error')}: {error.get('message', 'request failed')}")
    return response


def _read_daemon_frame(repo_root: Path, task: str, cap: int, generation: str | None) -> dict[str, Any]:
    return _read_daemon_request(repo_root, "orient", {"task": task, "limit": cap}, generation)


def manifest_digest(repo_root: Path) -> str:
    response = _read_daemon_request(repo_root, "status", {}, None)
    manifest = ((response.get("result") or {}).get("manifest") or {})
    digest = str(manifest.get("manifestDigest") or "")
    if digest.startswith("sha256:") and len(digest) == 71:
        return digest.lower()
    if not manifest:
        raise FileNotFoundError("Blueprint daemon returned no sealed manifest")
    canonical = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(canonical).hexdigest()


def _warning(kind: str, message: str) -> dict:
    return {"provider": "blueprint", "kind": kind, "severity": "warning", "message": message[:400]}


def _produce(repo_root: Path, task: str, max_tokens: int, observability: dict[str, Any], expected_generation: str | None = None) -> tuple[list[dict], str, list[dict]]:
    cap = candidate_cap(max_tokens)
    cache_key = _cache_key(repo_root, expected_generation, task, cap) if expected_generation else None
    if cache_key:
        cached = _cached_candidates(cache_key)
        if cached is not None:
            return cached, expected_generation, []
    started = time.monotonic()
    try:
        response = _read_daemon_frame(repo_root, task, cap, expected_generation)
    except (TimeoutError, socket.timeout):
        return [], "blueprint-timeout", [_warning("provider_timeout", f"Blueprint daemon exceeded {BLUEPRINT_TIMEOUT_S:.2f}s deadline")]
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        return [], "blueprint-unavailable", [_warning("blueprint_daemon_unavailable", str(exc))]
    observability["stageElapsedMs"] = {"blueprint_daemon": round(max(0.0, (time.monotonic() - started) * 1000), 6)}
    result = response.get("result") or {}
    generation = str(response.get("generation") or result.get("generationId") or expected_generation or "")
    if expected_generation and generation != expected_generation:
        return [], generation or "blueprint-generation-mismatch", [_warning("generation_mismatch", f"expected {expected_generation}; observed {generation}")]
    candidate_set = result.get("candidateSet") or {}
    candidates = [dict(candidate) for candidate in candidate_set.get("candidates") or []]
    circuit = result.get("recallCircuit") or {}
    warnings: list[dict] = []
    if circuit.get("state") == "abstained":
        warnings.append(_warning("blueprint_abstained_no_relevant_seed", "fresh graph has no relevant seed for task"))
    if cache_key and candidates:
        _cache_candidates(cache_key, candidates)
    return candidates, generation, warnings


def produce_with_observability(repo_root: Path, task: str, max_tokens: int, *, expected_generation: str | None = None) -> tuple[list[dict], str, list[dict], dict[str, Any]]:
    observability: dict[str, Any] = {"stageElapsedMs": {}}
    return (*_produce(repo_root, task, max_tokens, observability, expected_generation), observability)


def produce(repo_root: Path, task: str, max_tokens: int, *, expected_generation: str | None = None) -> tuple[list[dict], str, list[dict]]:
    observability: dict[str, Any] = {"stageElapsedMs": {}}
    return _produce(repo_root, task, max_tokens, observability, expected_generation)
