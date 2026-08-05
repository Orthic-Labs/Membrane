"""Cortex provider — spawns `cortex.mjs graph candidates` and parses output.

Reads a real ContextCandidateSet v1 produced by the existing static
provider (Cortex graph is the live machine-local generation). Output
candidates carry provider="cortex" and sourceGeneration = the
manifest.generationId.
"""
from __future__ import annotations

import json
import hashlib
import math
import os
import re
import sqlite3
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

from . import workspace_tools_path

CORTEX_CLI_DEFAULT = Path(__file__).resolve().parents[4] / "cortex/scripts/cortex.mjs"
CORTEX_SKILLS_CLI_DEFAULT = workspace_tools_path(
    "skills", "cortex", "scripts", "cortex.mjs"
)
CORTEX_CANDIDATE_CAP_DEFAULT = 64
# Cortex runs inside the gateway's one absolute 350 ms fan-out deadline.
# A shorter private cutoff discarded a valid graph result while time remained
# in that same budget, then reported a false stale generation.
CORTEX_TIMEOUT_S = 0.35
CORTEX_CACHE_TTL_S = 300.0
CORTEX_CACHE_MAX_ENTRIES = 16

# A successful candidate query can be reused only for its exact task, cap &
# graph generation. This keeps a brief Node-spawn miss from discarding an
# already-attested Cortex answer, without treating manifest metadata as code
# context or crossing a graph/source transition.
_candidate_cache: dict[tuple[str, str, str, int], tuple[float, list[dict]]] = {}
_candidate_cache_lock = threading.Lock()
_QUERY_STOPWORDS = frozenset({
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is",
    "it", "of", "on", "or", "that", "the", "this", "to", "verify", "with",
    "implement", "implementation", "execution", "harness",
})


def _cache_key(repo_root: Path, generation: str, task: str, cap: int) -> tuple[str, str, str, int]:
    return (
        str(repo_root.resolve()),
        generation,
        hashlib.sha256(task.encode("utf-8")).hexdigest(),
        cap,
    )


def _cached_candidates(key: tuple[str, str, str, int]) -> list[dict] | None:
    now = time.monotonic()
    with _candidate_cache_lock:
        expired = [entry for entry, (until, _value) in _candidate_cache.items() if until <= now]
        for entry in expired:
            _candidate_cache.pop(entry, None)
        cached = _candidate_cache.get(key)
        if cached is None:
            return None
        return [dict(candidate) for candidate in cached[1]]


def _cache_candidates(
    key: tuple[str, str, str, int], candidates: list[dict], *, now: float | None = None
) -> None:
    now = time.monotonic() if now is None else now
    with _candidate_cache_lock:
        if len(_candidate_cache) >= CORTEX_CACHE_MAX_ENTRIES:
            oldest = min(_candidate_cache, key=lambda entry: _candidate_cache[entry][0])
            _candidate_cache.pop(oldest, None)
        _candidate_cache[key] = (
            now + CORTEX_CACHE_TTL_S,
            [dict(candidate) for candidate in candidates],
        )


def candidate_cap(max_tokens: int, raw_override: str | None = None) -> int:
    """Bound repo-code candidate generation independently of the total token budget."""
    raw = raw_override if raw_override is not None else os.environ.get("MEMBRANE_CORTEX_CAP")
    try:
        configured = int(raw) if raw is not None else CORTEX_CANDIDATE_CAP_DEFAULT
    except ValueError:
        configured = CORTEX_CANDIDATE_CAP_DEFAULT
    return min(max(1, configured), 256, max(1, max_tokens))


def _resolve_cortex_cli() -> str:
    """Locate the Cortex CLI on this host.

    Honour an explicit CORTEX_CLI override (CI / dev override) and
    otherwise locate `cortex.mjs` relative to this file. Return the
    resolved path; raise FileNotFoundError if missing.
    """
    explicit = os.environ.get("CORTEX_CLI")
    if explicit:
        return explicit
    for candidate in (CORTEX_CLI_DEFAULT, CORTEX_SKILLS_CLI_DEFAULT):
        if candidate.exists():
            return str(candidate)
    return str(CORTEX_CLI_DEFAULT)


def _resolve_node() -> str:
    """Locate a Node.js binary that can run `cortex.mjs`.

    Honour NODE_BIN, then PATH lookup, then a Windows `node.exe` and
    POSIX `node` fallback. Raise FileNotFoundError if missing.
    """
    env_override = os.environ.get("NODE_BIN")
    if env_override and Path(env_override).exists():
        return env_override
    for name in ("node.exe", "node"):
        found = shutil.which(name)
        if found:
            return found
    # Last-resort fallback for hosts where the Node binary is installed
    # but not on PATH (Windows installer often drops it under Program Files).
    for guess in (
        r"C:\Program Files\nodejs\node.exe",
        r"C:\Program Files (x86)\nodejs\node.exe",
        "/usr/bin/node",
        "/usr/local/bin/node",
        "/opt/homebrew/bin/node",
    ):
        if Path(guess).exists():
            return guess
    raise FileNotFoundError(
        "node.js not found on PATH; install node and retry, or set NODE_BIN"
    )


def _connect_graph_db(graph_db: Path) -> sqlite3.Connection:
    """Open the Cortex store for reading, never writing to it.

    A WAL database opened `mode=ro` needs an existing `-shm`, which a
    read-only connection may not create. `cortex graph build` checkpoints
    those sidecars away, so a freshly built store is unopenable read-only and
    the lane degraded exactly when the graph was most current. Retry with
    `immutable=1`, which needs no shared-memory file. Read-only is tried first
    so an existing WAL is still honoured.
    """
    uri = graph_db.as_uri()
    try:
        connection = sqlite3.connect(f"{uri}?mode=ro", uri=True, timeout=0.05)
        # sqlite3.connect is lazy: a WAL store missing its -shm only fails on
        # first use, so force the open here or the fallback never fires.
        connection.execute("SELECT 1").fetchone()
        return connection
    except sqlite3.Error:
        return sqlite3.connect(f"{uri}?immutable=1", uri=True, timeout=0.05)


def _read_manifest(repo_root: Path) -> dict | None:
    """Read the sealed Cortex generation, preferring the graph.db envelope.

    Cortex's current store is SQLite. JSON manifests remain a compatibility
    fallback for older repositories. The provider uses only generation
    identity; the central Rust ``/freshness`` verdict owns freshness
    classification.
    """
    graph_db = repo_root / ".agent" / "graph" / "graph.db"
    if graph_db.exists():
        try:
            with _connect_graph_db(graph_db) as connection:
                row = connection.execute(
                    "SELECT value FROM generation WHERE key = 'manifest'"
                ).fetchone()
                if row is not None:
                    manifest = json.loads(row[0])
                    if isinstance(manifest, dict) and manifest.get("generationId"):
                        source = connection.execute(
                            "SELECT value FROM generation WHERE key = 'sourceObservation'"
                        ).fetchone()
                        if source is not None:
                            observation = json.loads(source[0])
                            if isinstance(observation, dict):
                                manifest.setdefault("baseCommit", observation.get("head"))
                                manifest.setdefault("sourceState", "dirty" if observation.get("dirty") else "clean")
                        return manifest
        except (OSError, sqlite3.Error, TypeError, ValueError):
            # A present but unreadable DB must not make the provider invent a
            # generation. Legacy JSON is retained for older Cortex repos.
            pass

    candidates = [
        repo_root / ".agent" / "graph" / "manifest.json",
        repo_root / ".agent" / "manifest.json",
    ]
    for path in candidates:
        if path.exists():
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                return None
    return None


def _cortex_warning(reason_kind: str, message: str) -> dict:
    return {
        "provider": "cortex",
        "kind": reason_kind,
        "severity": "warning",
        "message": message[:400],
    }


def _query_tokens(task: str) -> list[str]:
    """Small deterministic lexical query; task text never becomes SQL."""
    tokens: list[str] = []
    for raw in re.findall(r"[a-z0-9_][a-z0-9_-]{1,63}", task.lower()):
        token = raw.replace("-", "_")
        if token in _QUERY_STOPWORDS or token in tokens:
            continue
        tokens.append(token)
    return sorted(tokens, key=lambda token: (-len(token), token))[:8]


_ZERO_SHA256 = "sha256:" + "0" * 64


def _normalise_source_hash(value: object) -> str:
    """Plan 3.4 hash prefix: return a `sha256:`-prefixed 64-char hex digest.

    Accepts the modern `sha256:<64hex>` shape (live.py / gateway.py:92-96),
    a bare 64-hex digest, or the legacy 32-char unprefixed form. Any other
    shape collapses to the zero digest so consumers always see a uniformly
    self-describing, correctly-widthed identifier.
    """
    raw = str(value or "")
    if raw.startswith("sha256:") and len(raw.removeprefix("sha256:")) == 64:
        normalised = raw.removeprefix("sha256:").lower()
        if re.fullmatch(r"[0-9a-f]{64}", normalised):
            return "sha256:" + normalised
    if re.fullmatch(r"[0-9a-fA-F]{64}", raw):
        return "sha256:" + raw.lower()
    if re.fullmatch(r"[0-9a-fA-F]{32}", raw):
        return "sha256:" + raw.lower().ljust(64, "0")
    return _ZERO_SHA256


def _direct_index_candidates(
    repo_root: Path, task: str, cap: int, generation_id: str
) -> list[dict] | None:
    """Query a centrally pinned graph generation without Node startup.

    Crypt `/freshness` owns source-tree validation and passes its exact generation.
    This lane verifies that pin against graph.db, reads only sealed symbol/file rows,
    and returns no result when the store cannot satisfy that contract.
    """
    graph_db = repo_root / ".agent" / "graph" / "graph.db"
    tokens = _query_tokens(task)
    if not graph_db.exists() or not generation_id or not tokens:
        return None
    try:
        with _connect_graph_db(graph_db) as connection:
            manifest_row = connection.execute(
                "SELECT value FROM generation WHERE key='manifest'"
            ).fetchone()
            if manifest_row is None:
                return None
            manifest = json.loads(manifest_row[0])
            if not isinstance(manifest, dict) or manifest.get("generationId") != generation_id:
                return None
            search_row = connection.execute(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='symbol_search'"
            ).fetchone()
            has_terms = connection.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='symbol_terms'"
            ).fetchone() is not None
            has_search = search_row is not None
            search_is_fts = has_search and "fts5" in str(search_row[0] or "").lower()
            symbols: list[tuple] = []
            score_tokens = tokens
            if has_terms:
                for token in tokens:
                    symbols = connection.execute(
                        "SELECT s.id,s.name,s.qualified_name,s.path,s.confidence,s.evidence "
                        "FROM symbol_terms AS st JOIN symbols AS s "
                        "ON s.id=st.symbol_id AND s.generation_id=st.generation_id "
                        "WHERE st.generation_id=? AND st.token=? LIMIT ?",
                        (generation_id, token, cap),
                    ).fetchall()
                    if symbols:
                        score_tokens = [token]
                        break
            elif has_search:
                # Cortex owns this generation-sealed index. Task text is
                # normalized before becoming a bound value; it never becomes
                # SQL. Tokens are grammar-safe for the FTS prefix query.
                if search_is_fts:
                    match = " OR ".join(f"{token}*" for token in tokens)
                    symbols = connection.execute(
                        "SELECT s.id,s.name,s.qualified_name,s.path,s.confidence,s.evidence "
                        "FROM symbol_search AS ss JOIN symbols AS s "
                        "ON s.id=ss.id AND s.generation_id=ss.generation_id "
                        "WHERE ss.generation_id=? AND symbol_search MATCH ? "
                        "ORDER BY bm25(symbol_search), s.confidence DESC, s.path, s.id LIMIT ?",
                        (generation_id, match, cap),
                    ).fetchall()
                else:
                    term = "lower(ss.name || ' ' || ss.qualified_name || ' ' || ss.path)"
                    where = " OR ".join(f"instr({term}, ?) > 0" for _ in tokens)
                    symbols = connection.execute(
                        "SELECT s.id,s.name,s.qualified_name,s.path,s.confidence,s.evidence "
                        "FROM symbol_search AS ss JOIN symbols AS s "
                        "ON s.id=ss.id AND s.generation_id=ss.generation_id "
                        f"WHERE ss.generation_id=? AND ({where}) "
                        "ORDER BY s.confidence DESC, s.path, s.id LIMIT ?",
                        (generation_id, *tokens, cap),
                    ).fetchall()
            else:
                # Compatibility for graph stores created before Cortex shipped
                # symbol_search. This path disappears after the next graph build.
                term = "lower(coalesce(name,'') || ' ' || coalesce(qualified_name,'') || ' ' || coalesce(path,''))"
                where = " OR ".join(f"instr({term}, ?) > 0" for _ in tokens)
                symbols = connection.execute(
                    "SELECT id,name,qualified_name,path,confidence,evidence "
                    f"FROM symbols WHERE generation_id=? AND ({where}) LIMIT ?",
                    [generation_id, *tokens, cap],
                ).fetchall()
            if not symbols:
                # A current graph with no lexical hit is the canonical case for
                # abstention. Return an empty result so `_produce` can publish
                # the plan 3.3 {abstained, reason: "no_relevant_seed"} signal
                # instead of the retired arbitrary cohort.
                pass
    except (OSError, sqlite3.Error, TypeError, ValueError, json.JSONDecodeError):
        return None

    ranked: list[tuple[int, float, str, str, dict]] = []
    for row in symbols:
        entry_id, name, qualified_name, path, confidence, evidence_json = row
        if not entry_id or not path:
            continue
        try:
            evidence = json.loads(evidence_json or "[]")
        except (TypeError, json.JSONDecodeError):
            continue
        if not isinstance(evidence, list) or not evidence or not isinstance(evidence[0], dict):
            continue
        first = evidence[0]
        start = max(1, int(first.get("startLine") or 1))
        end = max(start, int(first.get("endLine") or start))
        # Plan 3.4 hash prefix: every emitted sourceHash is a self-describing
        # `sha256:` hex digest at the same 64-character width already
        # validated by gateway.py and emitted by live.py.
        content_hash = _normalise_source_hash(first.get("contentHash") or "")
        # Exactness matches Cortex (cortex/graph/static-provider.mjs:538-539):
        # a symbol is exact only if its full lowercased qualified_name OR its
        # last dotted segment equals one of the query tokens. The token set is
        # already trimmed to a single winning term when symbol_terms exists.
        qualified = str(qualified_name or name or "").lower()
        short_name = qualified.rsplit(".", 1)[-1] if qualified else ""
        lexical_score = sum(
            1 for token in score_tokens if token == qualified or token == short_name
        )
        candidate = {
            "id": f"cortex:{entry_id}",
            "layer": 3,
            "sourceKind": "repo_code",
            "sourceRef": f"{path}:{start}-{end}",
            "sourceHash": content_hash,
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": 1.0 if lexical_score == len(score_tokens) else 0.0,
            "scoreComponents": {"lexical": float(lexical_score)},
            "estimatedTokens": min(256, max(16, (end - start + 1) * 8)),
            "protected": False,
            "exact": bool(score_tokens) and lexical_score == len(score_tokens),
            "recoverable": True,
            # Plan 3.4 resolver drift: candidates originated from the
            # SQLite-pinned read, never from executing `graph resolve`. The
            # published resolver must therefore name the same command-line
            # shape that the source provider actually executes on a re-fetch
            # (cortex/scripts/cortex.mjs `graph resolve --node <id>`).
            "resolver": f"cortex graph resolve --node {entry_id}",
            "text": str(qualified_name or name or entry_id),
        }
        ranked.append((-lexical_score, -float(confidence or 0.0), str(path), str(entry_id), candidate))
    ranked.sort(key=lambda item: item[:4])
    seen: set[str] = set()
    output: list[dict] = []
    for *_rank, candidate in ranked:
        key = candidate["sourceRef"]
        if key in seen:
            continue
        seen.add(key)
        output.append(candidate)
        if len(output) == cap:
            break
    return output


def _produce(
    repo_root: Path,
    task: str,
    max_tokens: int,
    observability: dict[str, Any],
    expected_generation: str | None = None,
) -> tuple[list[dict], str, list[dict]]:
    """Plan 3.3 contract:

    `(candidates, generation, warnings)` is the established 3-tuple return
    shape retained for callers. The abstention signal is attached to the
    observability dict already passed in (option B from plan 3.3) under the
    key ``abstention`` so gateway.py can promote it to its own typed envelope
    without breaking the existing tuple contract.

    Abstention triggers ONLY when all of the following hold — every other
    provider failure mode degrades to its prior empty-list path with a
    typed warning:

    * the on-disk generation matches the centrally validated one (so the
      graph is CURRENT, not stale);
    * the SQLite-pinned direct index produced no candidates (no lexical
      evidence under name equality).
    """
    warnings: list[dict] = []
    cli = _resolve_cortex_cli()
    if not Path(cli).exists():
        warnings.append(_cortex_warning("cortex_cli_missing", f"cortex CLI missing at {cli}"))
        return [], "cortex-missing", warnings
    node_bin = _resolve_node()
    manifest = _read_manifest(repo_root)
    generation_id = (manifest or {}).get("generationId") or ""
    cap = candidate_cap(max_tokens)
    cache_key = None
    if generation_id and (not expected_generation or generation_id == expected_generation):
        cache_key = _cache_key(repo_root, generation_id, task, cap)
    if expected_generation and generation_id == expected_generation:
        direct = _direct_index_candidates(repo_root, task, cap, expected_generation)
        if direct is not None:
            observability["stageElapsedMs"] = {"repo_code_scan": 0.0}
            if not direct:
                # Pinned graph + zero lexical evidence => abstention. Plan 3.3
                # contract: callers (gateway.py via produce_with_observability)
                # surface abstained=True and reason="no_relevant_seed"; the
                # generation id is still authoritative so downstream adapters
                # can distinguish "no evidence" from "graph missing".
                abstention = {
                    "abstained": True,
                    "reason": "no_relevant_seed",
                    "candidates": [],
                    "generationId": generation_id,
                }
                observability["abstention"] = abstention
                warnings.append(_cortex_warning(
                    # Convention 4: agent-visible reasons say Cortex, never the
                    # retired Cortex vocabulary. The abstention fix itself
                    # introduced a `cortex_*` reason string — retiring a term
                    # while emitting a new instance of it.
                    "cortex_abstained_no_relevant_seed",
                    f"fresh graph {generation_id} has no relevant seed for task",
                ))
                if cache_key is not None:
                    _cache_candidates(cache_key, direct)
                return [], generation_id, warnings
            if cache_key is not None:
                _cache_candidates(cache_key, direct)
            return direct, generation_id, warnings
    cmd = [
        node_bin,
        cli,
        "graph", "candidates",
        "--task", task,
        "--out", ".agent",
        "--limit", str(cap),
    ]
    if expected_generation:
        cmd.extend(["--expected-generation", expected_generation])
    subprocess_started = time.monotonic()
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(repo_root),
            capture_output=True,
            text=True,
            timeout=CORTEX_TIMEOUT_S,
            check=False,
        )
    except subprocess.TimeoutExpired:
        if cache_key is not None:
            cached = _cached_candidates(cache_key)
            if cached is not None:
                return cached, generation_id, warnings
        warnings.append(
            _cortex_warning(
                "provider_timeout",
                f"cortex exceeded {CORTEX_TIMEOUT_S:.2f}s process deadline",
            )
        )
        return [], "cortex-timeout", warnings
    subprocess_finished = time.monotonic()
    subprocess_elapsed_ms = max(0.0, (subprocess_finished - subprocess_started) * 1000.0)
    observability["stageElapsedMs"] = {
        "cortex_node_spawn": round(subprocess_elapsed_ms, 6),
    }
    if proc.returncode != 0:
        warnings.append(_cortex_warning("cortex_nonzero_exit", f"exit={proc.returncode}; stderr={proc.stderr.strip()[:300]}"))
        return [], "cortex-failed", warnings
    out = proc.stdout.strip()
    if not out:
        warnings.append(_cortex_warning("cortex_empty_output", "empty stdout"))
        return [], "cortex-failed", warnings
    try:
        payload = json.loads(out)
    except json.JSONDecodeError as exc:
        warnings.append(_cortex_warning("cortex_json_decode", str(exc)[:300]))
        return [], "cortex-failed", warnings
    raw_membrane = payload.get("_membrane") if isinstance(payload, dict) else None
    raw_stages = raw_membrane.get("stageElapsedMs") if isinstance(raw_membrane, dict) else None
    raw_repo_scan = raw_stages.get("repo_code_scan") if isinstance(raw_stages, dict) else None
    if (
        isinstance(raw_repo_scan, (int, float))
        and not isinstance(raw_repo_scan, bool)
        and math.isfinite(float(raw_repo_scan))
        and float(raw_repo_scan) >= 0
    ):
        repo_scan_ms = min(float(raw_repo_scan), subprocess_elapsed_ms)
        observability["stageElapsedMs"] = {
            "cortex_node_spawn": round(subprocess_elapsed_ms - repo_scan_ms, 6),
            "repo_code_scan": round(repo_scan_ms, 6),
        }
    # `graph candidates` returns a full ContextCandidateSet v1 object
    # (schemaVersion + candidates[...]). Normalise to the candidate list.
    if isinstance(payload, dict) and isinstance(payload.get("candidates"), list):
        results = payload["candidates"]
    elif isinstance(payload, list):
        results = payload
    else:
        warnings.append(_cortex_warning("cortex_unexpected_shape", f"unexpected top-level shape; first 200 bytes: {out[:200]}"))
        return [], "cortex-failed", warnings

    candidates: list[dict[str, Any]] = []
    for entry in results:
        # Cortex CLI may emit candidates in two shapes:
        # 1. Top-level (modern): sourceRef/sourceHash at root, no `evidence[]`
        # 2. Evidence-wrapped (legacy): each candidate has `evidence[]` with
        #    path/startLine/endLine/contentHash.
        # We support BOTH. When evidence is present, prefer its path lines;
        # otherwise build sourceRef from top-level sourceRef + layer info.
        evidence_list = entry.get("evidence") or []
        entry_id = entry.get("id") or entry.get("qualifiedName") or ""
        if evidence_list:
            first_ev = evidence_list[0]
            ev_path = first_ev.get("path") or ""
            ev_start = int(first_ev.get("startLine", 1))
            ev_end = int(first_ev.get("endLine", ev_start))
            source_ref = f"{ev_path}:{ev_start}-{ev_end}"
            # Plan 3.4 hash prefix: every emitted sourceHash must be a
            # `sha256:`-prefixed 64-char hex digest (gateway.py:92-96),
            # never the legacy unprefixed `"0"*32`.
            raw_ev_hash = first_ev.get("contentHash") or entry.get("sourceHash") or ""
            source_hash = _normalise_source_hash(raw_ev_hash)
        else:
            source_ref = entry.get("sourceRef") or entry_id
            source_hash = _normalise_source_hash(entry.get("sourceHash") or "")
        if not source_ref:
            # Skip if neither shape carries a usable reference; surface
            # a ProviderWarning upstream instead of an empty candidate.
            continue
        declared_trust = entry.get("trustClass")
        candidates.append({
            "id": f"cortex:{entry_id}",
            "layer": int(entry.get("layer", 3)),
            "sourceKind": entry.get("sourceKind", "repo_code"),
            "sourceRef": source_ref,
            "sourceHash": source_hash,
            "trustClass": declared_trust or "workspace_tracked",
            "instructionPolicy": entry.get("instructionPolicy", "data_only"),
            "providerScore": float(entry.get("providerScore") or entry.get("confidence") or 0.0),
            "scoreComponents": dict(entry.get("scoreComponents") or {"lexical": 0.0}),
            "estimatedTokens": int(entry.get("estimatedTokens") or 100),
            "protected": bool(entry.get("protected", False)),
            "exact": bool(entry.get("exact", False)),
            "recoverable": bool(entry.get("recoverable", True)),
            # Plan 3.4 resolver drift: this lane actually executes
            # `cortex.mjs graph candidates`; the per-id re-fetch contract
            # is the same `cortex graph resolve --node <id>` shape used
            # by anchors.py:91 and Cortex at cortex/scripts/cortex.mjs:2501.
            "resolver": entry.get("resolver") or f"cortex graph resolve --node {entry_id}",
            "text": entry.get("qualifiedName") or entry.get("name") or entry_id,
        })
    generation_id = generation_id or "cortex-federation-stub"
    if cache_key is not None and generation_id == (expected_generation or generation_id):
        _cache_candidates(cache_key, candidates, now=subprocess_finished)
    return candidates, generation_id, warnings


def produce_with_observability(
    repo_root: Path,
    task: str,
    max_tokens: int,
    *,
    expected_generation: str | None = None,
) -> tuple[list[dict], str, list[dict], dict[str, Any]]:
    """Produce candidates plus bounded, content-free provider stage timings."""
    observability: dict[str, Any] = {"stageElapsedMs": {}}
    return (*_produce(
        repo_root, task, max_tokens, observability, expected_generation
    ), observability)


def produce(
    repo_root: Path,
    task: str,
    max_tokens: int,
    *,
    expected_generation: str | None = None,
) -> tuple[list[dict], str, list[dict]]:
    """Compatibility wrapper retaining the established three-tuple API."""
    observability: dict[str, Any] = {"stageElapsedMs": {}}
    return _produce(
        repo_root, task, max_tokens, observability, expected_generation
    )
