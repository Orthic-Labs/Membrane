#!/usr/bin/env py -3.11
"""RightContext federation gateway CLI (Python orchestration layer).

Subcommand: `py -3.11 membrane/engine/federation/gateway.py --task T --repo R
                    --max-tokens N --client C --session S
                    [--anchors a,b,c] [--scope-grant-id G]`

Writes a fully-assembled ContextCandidateSet v1 to stdout. The Rust
dispatcher (`crypt federate`) parses this, runs the deterministic
in-process admission, and emits the final envelope.

Hard rules (per dispatch):
  - Provider payload formats never enter client adapters.
  - Bearer token never appears in argv, stdout, log, or telemetry.
  - Single provider failure must NOT abort the federation; emit a loud
    ProviderWarning and continue with healthy providers.
  - Cross-root evidence requires a valid ScopeGrant (active, not expired).
    An invalid grant ABORTS the federation (fail-closed); a missing grant
    on a same-root request is permitted (per dispatch §G1 trust contract).
  - Raw prompt and repo content never enter receipts (they are
    content-free by construction).
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import queue
import sys
import threading
import time
import urllib.request
import uuid
from pathlib import Path
from typing import Any

# The Rust dispatcher (`crypt federate`) launches this script with cwd =
# the user's repo root, not the workspace. Make sure `federation.providers.*`
# is importable regardless of cwd by adding the script's PARENT (the workspace
# dir that contains the `federation` package) to sys.path.
_THIS_DIR = Path(__file__).resolve().parent
_PARENT = _THIS_DIR.parent
if str(_PARENT) not in sys.path:
    sys.path.insert(0, str(_PARENT))

# Local providers — all in-process or short subprocess. None of them touch
# the Crypt DB schema, all read-only or pure.
from federation.providers import blueprint, audit, architect, crypt, git_provider  # noqa: E402
from federation.providers import live, rules, anchors, scope_grant  # noqa: E402
from federation.providers import skills  # noqa: E402


_PROVIDER_STAGE_NAMES = frozenset({
    "request_parse", "embed", "recall", "rank", "blueprint_node_spawn", "repo_code_scan",
})
_LANE_FAILURE_KINDS = frozenset({
    "timeout", "crash", "unavailable", "authentication", "invalid_output",
    "stale_snapshot", "generation_mismatch", "cancellation_budget_drop", "circuit_open",
})
def _resolve_release_manifest() -> Path:
    """Locate tools/lib/crypt-release.json across both repo layouts.

    Before the membrane consolidation this script lived at
    `tools/crypt/federation/`, so `parents[1]/lib` WAS `tools/lib`. It now
    lives at `membrane/engine/federation/`, where that same expression points
    at a `membrane/lib` that holds no release manifest — which silently
    degraded every packet to `release_generation_unavailable`. Probe the real
    layouts instead of assuming a fixed depth; first hit wins.
    """
    override = os.environ.get("CRYPT_RELEASE_MANIFEST", "").strip()
    if override:
        return Path(override)
    candidates = (
        # membrane nested inside the parent workspace
        _THIS_DIR.parents[2] / "tools" / "lib" / "crypt-release.json",
        # pre-consolidation tools/crypt layout
        _THIS_DIR.parents[1] / "lib" / "crypt-release.json",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return candidates[0]


_RELEASE_MANIFEST = _resolve_release_manifest()
_PRODUCTION_FANOUT_TIMEOUT_S = 0.35
_REPLAY_FANOUT_TIMEOUT_S = 45.0


def _valid_release_generation(value: Any) -> bool:
    if not isinstance(value, str) or not value.startswith("sha256:"):
        return False
    digest = value.removeprefix("sha256:")
    return len(digest) == 64 and all(char in "0123456789abcdefABCDEF" for char in digest)


def _expected_release_generation() -> str | None:
    """Read one internally consistent release generation from the source manifest."""
    try:
        document = json.loads(_RELEASE_MANIFEST.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(document, dict):
        return None
    generation = document.get("release_generation")
    tree_digest = document.get("source_tree_sha256")
    if (
        not _valid_release_generation(generation)
        or not isinstance(tree_digest, str)
        or generation != f"sha256:{tree_digest}"
    ):
        return None
    return generation


def _release_generation_state(expected: Any, observed: Any) -> str:
    if not _valid_release_generation(expected) or not _valid_release_generation(observed):
        return "unavailable"
    return "matched" if expected == observed else "mismatch"


def _resolve_repo_root(repo_arg: str) -> Path:
    p = Path(repo_arg).resolve()
    if not p.is_dir():
        raise SystemExit(f"federation: --repo {repo_arg} is not a directory")
    return p


def _trace_id() -> str:
    return f"rc-fed-{int(time.time() * 1000):x}-{uuid.uuid4().hex[:8]}"


def _freshness_token(repo_root: Path) -> str:
    raw = os.environ.get("CRYPT_API_TOKEN", "").strip()
    if raw:
        return raw
    override = os.environ.get("CRYPT_API_TOKEN_FILE", "").strip()
    candidates = [Path(override)] if override else []
    workspace = os.environ.get("WORKSPACE_ROOT", "").strip()
    if workspace:
        candidates.append(Path(workspace) / "tools" / ".cache" / "memory" / "api-token")
    candidates.extend(
        parent / "tools" / ".cache" / "memory" / "api-token"
        for parent in (repo_root, *repo_root.parents)
    )
    for candidate in candidates:
        try:
            token = candidate.read_text(encoding="utf-8").strip()
        except OSError:
            continue
        if token:
            return token
    return ""


def _fetch_freshness_verdict(repo_root: Path) -> dict[str, Any] | None:
    """Fetch the sole freshness verdict from the resident service.

    The gateway deliberately has no Git/manifest fallback implementation. If the service cannot
    produce a typed verdict, the existing terminal legacy path remains the safe fallback.
    """
    port = os.environ.get("CRYPT_PORT") or os.environ.get("WORKSPACE_MEMORY_PORT") or "47851"
    token = _freshness_token(repo_root)
    body = json.dumps({"repoRoot": str(repo_root)}).encode("utf-8")
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/freshness",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"},
    )
    try:
        timeout = max(0.1, float(os.environ.get("RIGHTCONTEXT_FRESHNESS_TIMEOUT_S", "2.0")))
        with urllib.request.urlopen(request, timeout=timeout) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
    except (OSError, ValueError, json.JSONDecodeError):
        return None
    if not isinstance(payload, dict) or payload.get("schemaVersion") != 1:
        return None
    if not isinstance(payload.get("graphState"), str):
        return None
    return payload


def _safe_warning_token(value: Any, fallback: str) -> str:
    token = str(value or "").strip()
    if 0 < len(token) <= 80 and all(
        char.isascii() and (char.isalnum() or char in "._:-") for char in token
    ):
        return token
    return fallback


def _classify_lane_failure(*signals: Any) -> str:
    text = " ".join(str(signal or "").strip().lower().replace("-", "_") for signal in signals)
    if "generation" in text and any(word in text for word in ("changed", "mismatch")):
        return "generation_mismatch"
    if "stale_snapshot" in text or ("stale" in text and "snapshot" in text):
        return "stale_snapshot"
    if "circuit_open" in text or ("circuit" in text and "open" in text):
        return "circuit_open"
    if any(word in text for word in ("budget_drop", "budget drop", "cancelled", "canceled")):
        return "cancellation_budget_drop"
    if any(word in text for word in ("timeout", "timed_out", "deadline")):
        return "timeout"
    if any(word in text for word in ("authentication", "unauthorized", "forbidden", "credential")):
        return "authentication"
    if any(word in text for word in ("invalid_output", "malformed", "jsondecode", "parseerror")):
        return "invalid_output"
    if any(word in text for word in ("crash", "panic", "calledprocess", "processerror")):
        return "crash"
    return "unavailable"


def _normalize_provider_warning(provider_name: str, warning: Any) -> dict[str, Any]:
    raw = warning if isinstance(warning, dict) else {}
    kind = _safe_warning_token(raw.get("kind"), "provider_failure")
    explicit_failure = raw.get("failureKind")
    failure_kind = (
        explicit_failure
        if isinstance(explicit_failure, str) and explicit_failure in _LANE_FAILURE_KINDS
        else _classify_lane_failure(kind, raw.get("message"))
    )
    severity = raw.get("severity") if raw.get("severity") in {"warning", "blocker"} else "warning"
    return {
        "provider": _safe_warning_token(provider_name, "unknown"),
        "kind": kind,
        "severity": severity,
        "failureKind": failure_kind,
        "failureScope": "lane_local",
    }


def _safe_emit_warning(provider_name: str, exc: Exception) -> dict[str, Any]:
    """Provider-failure warning that is safe to surface in any output.

    Never includes the bearer token, the prompt, or repo content.
    """
    return _normalize_provider_warning(
        provider_name,
        {"kind": "provider_failure", "exceptionType": type(exc).__name__,
         "failureKind": _classify_lane_failure(type(exc).__name__)},
    )


def _fanout_timeout_seconds() -> float:
    if (
        os.environ.get("REPLAY_NO_LEGACY", "").strip() == "1"
        and os.environ.get("RIGHTCONTEXT_SAMPLE_SOURCE", "").strip().lower()
        in {"replay", "test"}
    ):
        return _REPLAY_FANOUT_TIMEOUT_S
    try:
        configured = float(
            os.environ.get(
                "RIGHTCONTEXT_PROVIDER_FANOUT_TIMEOUT_S",
                str(_PRODUCTION_FANOUT_TIMEOUT_S),
            )
        )
    except (TypeError, ValueError):
        configured = _PRODUCTION_FANOUT_TIMEOUT_S
    return min(_PRODUCTION_FANOUT_TIMEOUT_S, max(0.05, configured))


def _collect_tasks_bounded(
    tasks: list[tuple[str, Any]], *, timeout_s: float
) -> list[tuple[str, list[dict], list[dict]]]:
    """Collect partial provider results under one absolute prompt deadline.

    Daemon workers ensure an uncooperative read-only provider cannot keep the short-lived gateway
    process alive. Every unfinished lane receives a typed terminal warning, so a timeout is never
    mistaken for an empty successful provider.
    """
    completed: queue.Queue[tuple[str, tuple[str, list[dict], list[dict]]]] = queue.Queue()
    pending = {name for name, _task in tasks}

    def run(name: str, task: Any) -> None:
        try:
            result = task()
        except Exception as exc:  # provider isolation is the contract
            result = (name, [], [_safe_emit_warning(name, exc)])
        completed.put((name, result))

    for name, task in tasks:
        threading.Thread(target=run, args=(name, task), daemon=True).start()

    deadline = time.monotonic() + max(0.0, timeout_s)
    out: list[tuple[str, list[dict], list[dict]]] = []
    while pending:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        try:
            declared_name, result = completed.get(timeout=remaining)
        except queue.Empty:
            break
        if declared_name not in pending:
            continue
        pending.remove(declared_name)
        out.append(result)

    for name in sorted(pending):
        out.append(
            (
                name,
                [],
                [
                    {
                        "provider": name,
                        "kind": "provider_timeout",
                        "severity": "warning",
                        "message": "provider exceeded prompt fanout deadline",
                    }
                ],
            )
        )
    return out


# ---------------------------------------------------------------------------
# ScopeGrant fail-closed enforcement (dispatch §G1 trust contract).
# ---------------------------------------------------------------------------


def _manifest_digest(repo_root: Path) -> str:
    manifest_path = repo_root / ".blueprint" / "manifest.json"
    try:
        content = manifest_path.read_bytes()
    except OSError as exc:
        raise PermissionError(
            f"scope_grant_invalid: blueprint manifest unavailable at {manifest_path}"
        ) from exc
    return "sha256:" + hashlib.sha256(content).hexdigest()


def _enforce_scope_grant(
    repo_root: Path,
    scope_grant_id: str | None,
    *,
    client: str,
    task: str,
    session: str | None,
) -> dict | None:
    """Return the active grant or ABORT the federation.

    The dispatch §G1 trust contract says the gateway is the sole issuer of
    ScopeGrants, and that cross-root evidence is conditional on an active
    grant. An invalid / expired / revoked grant ABORTS the federation with a
    loud fail-closed error; the planner never sees the partially-assembled
    CCS. A missing grant on a SAME-root request is permitted (the planner's
    same-root short-circuit still admits workspace_tracked evidence).

    Returns the grant dict on success; raises PermissionError on failure.
    """
    if not scope_grant_id:
        return None  # same-root allowed without a grant
    if not session:
        raise PermissionError(
            "scope_grant_invalid: request session is required when a grant is supplied"
        )
    try:
        grant = scope_grant.lookup(repo_root, scope_grant_id)
    except Exception as exc:
        raise PermissionError(
            f"scope_grant_lookup_failed: {scope_grant_id}: {type(exc).__name__}: {str(exc)[:200]}"
        ) from exc
    if grant is None:
        raise PermissionError(
            f"scope_grant_missing: {scope_grant_id} not found in catalog (or unreadable)"
        )
    if grant.get("status") != "active":
        raise PermissionError(
            f"scope_grant_inactive: {scope_grant_id} status={grant.get('status')!r}"
        )
    # The gateway is invoked with (client, session) on the command line;
    # task is the planner task. Validate the grant binds to exactly
    # these values per dispatch §G1 trust contract. The repo_root is
    # already on disk; we pass it as the binding target.
    scope_grant.validate(
        grant,
        request_client=client,
        request_repo_root=repo_root,
        request_task=task,
        request_session=session,
        request_manifest_digest=_manifest_digest(repo_root),
    )
    return grant


# ---------------------------------------------------------------------------
# Parallel provider fan-out (one failure does NOT abort the federation).
# ---------------------------------------------------------------------------


def _invoke_provider(name: str, fn, *args) -> tuple[str, list[dict], list[dict]]:
    """Invoke one provider; return (name, candidates, warnings).

    A single provider failure is captured as a warning tuple, not raised.
    This is the dispatch §G3B / §G5 fault-isolation guarantee: the
    federation continues with whatever subset of providers succeeded.
    """
    try:
        candidates, _generation = fn(*args)
        return (name, candidates, [])
    except Exception as exc:
        return (name, [], [_safe_emit_warning(name, exc)])


def _gather_all_parallel(
    *,
    task: str,
    repo_root: Path,
    max_tokens: int,
    explicit_anchors: list[str],
    scope_grant_id: str | None,
    scope_descriptor: dict | None = None,
) -> tuple[list[tuple[str, list[dict[str, Any]], list[dict[str, Any]]]], dict[str, Any]]:
    """Invoke every provider in PARALLEL; collect candidates + warnings.

    The 9 providers run concurrently in a thread pool. A single
    provider failure MUST NOT abort the federation — it surfaces as a
    ProviderWarning on the envelope and the federation continues with
    whatever subset of providers succeeded.
    """
    indexed_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    expected_release_generation = _expected_release_generation()
    freshness_started = time.monotonic()
    verdict = _fetch_freshness_verdict(repo_root)
    freshness_elapsed_ms = max(0.0, (time.monotonic() - freshness_started) * 1000.0)
    if verdict is None:
        # No second Git/manifest implementation lives here. A missing central verdict is the one
        # condition that retains the packet-wide legacy fallback.
        freshness: dict[str, Any] = {
            "indexedAt": indexed_at,
            "stale": True,
            "graphState": "indeterminate",
            "expectedReleaseGeneration": expected_release_generation,
            "observedReleaseGeneration": None,
            "releaseGenerationStatus": "unavailable",
            "_stageElapsedMs": {"freshness": freshness_elapsed_ms},
        }
        return [], freshness
    observed_release_generation = verdict.get("releaseGeneration")
    release_generation_status = _release_generation_state(
        expected_release_generation, observed_release_generation
    )
    providers_state = verdict.get("providers") if isinstance(verdict.get("providers"), dict) else {}
    central_stage_elapsed_ms: dict[str, float] = {}
    raw_central_stages = verdict.get("stageElapsedMs")
    raw_git_status = (
        raw_central_stages.get("git_status")
        if isinstance(raw_central_stages, dict)
        else None
    )
    if (
        isinstance(raw_git_status, (int, float))
        and not isinstance(raw_git_status, bool)
        and 0 <= float(raw_git_status) < float("inf")
    ):
        central_stage_elapsed_ms["git_status"] = float(raw_git_status)
    freshness = {
        "indexedAt": str(verdict.get("checkedAt") or indexed_at),
        "stale": False,
        "graphState": str(verdict.get("graphState") or "indeterminate"),
        "snapshotId": verdict.get("snapshotId"),
        "baseCommit": verdict.get("baseCommit"),
        "blueprintBaseCommit": verdict.get("blueprintBaseCommit"),
        "overlayDigest": verdict.get("overlayDigest"),
        "serviceGeneration": verdict.get("serviceGeneration"),
        "expectedReleaseGeneration": expected_release_generation,
        "observedReleaseGeneration": observed_release_generation,
        "releaseGenerationStatus": release_generation_status,
        "firstAfterIdle": bool(verdict.get("firstAfterIdle", False)),
        "idleGapMs": verdict.get("idleGapMs"),
        "cacheAgeMs": verdict.get("cacheAgeMs"),
        "refreshInFlight": bool(verdict.get("refreshInFlight", False)),
        "_providerFreshness": {
            "blueprint": (providers_state.get("blueprint") or {}).get("freshnessClass"),
            "live": (providers_state.get("dirtyOverlay") or {}).get("freshnessClass"),
            "skills": (providers_state.get("skills") or {}).get("freshnessClass"),
        },
        "_providerBaseCommit": {
            "blueprint": verdict.get("blueprintBaseCommit"),
            "live": verdict.get("baseCommit"),
        },
    }
    if release_generation_status != "matched":
        freshness["stale"] = True
        return [], freshness
    provider_elapsed_ms: dict[str, float] = {}
    provider_stage_elapsed_ms: dict[str, dict[str, float]] = {}
    expected_blueprint_generation = str(verdict.get("blueprintGeneration") or "")
    expected_skills_generation = str(verdict.get("skillsGeneration") or "")

    # Build the list of (name, callable, args) — the actual fan-out.
    # Providers have different return shapes: most return (candidates,
    # generation); some return just the candidates list. The dispatcher
    # below normalises both shapes to (candidates, generation, generation_id).
    def _adapter(
        name: str, fn, *args, **kwargs
    ) -> tuple[str, list[dict[str, Any]], list[dict[str, Any]]]:
        """Adapter: invoke provider, normalise return shape.

        Returns (provider_name, candidates, warnings). On exception, the
        warning list carries the typed provider_failure record so the
        dispatcher can surface it on the envelope — never silently dropped.
        """
        started = time.monotonic()
        try:
            result = fn(*args, **kwargs)
        except Exception as exc:
            provider_elapsed_ms[name] = max(0.0, (time.monotonic() - started) * 1000.0)
            return (name, [], [_safe_emit_warning(name, exc)])
        provider_elapsed_ms[name] = max(0.0, (time.monotonic() - started) * 1000.0)
        warnings: list[dict[str, Any]] = []
        generation: Any = None
        raw_observability: Any = None
        if isinstance(result, tuple) and len(result) == 4:
            cands = result[0]
            generation = result[1]
            warnings = list(result[2] or [])
            raw_observability = result[3]
        elif isinstance(result, tuple) and len(result) == 3:
            cands = result[0]
            generation = result[1]
            if isinstance(result[2], dict):
                raw_observability = result[2]
            else:
                # Real provider signature: (candidates, generation, warnings).
                warnings = list(result[2] or [])
        elif isinstance(result, tuple) and len(result) == 2:
            cands, generation = result
        else:
            cands = list(result) if result else []
        if isinstance(raw_observability, dict):
            raw_stages = raw_observability.get("stageElapsedMs")
            if isinstance(raw_stages, dict):
                stages: dict[str, float] = {}
                for stage, elapsed in raw_stages.items():
                    if stage not in _PROVIDER_STAGE_NAMES or isinstance(elapsed, bool):
                        continue
                    if isinstance(elapsed, (int, float)):
                        numeric = float(elapsed)
                        if numeric >= 0 and numeric < float("inf"):
                            stages[stage] = numeric
                if stages:
                    provider_stage_elapsed_ms[name] = stages
        if name == "blueprint" and str(generation or "") != expected_blueprint_generation:
            return (
                name,
                [],
                [
                    {
                        "provider": "blueprint",
                        "kind": "blueprint_generation_changed",
                        "severity": "warning",
                        "message": "blueprint generation changed after freshness verdict",
                    }
                ],
            )
        if name == "skills" and str(generation or "") != expected_skills_generation:
            return (
                name,
                [],
                [
                    {
                        "provider": "skills",
                        "kind": "skills_generation_changed",
                        "severity": "warning",
                        "message": "skills generation changed after freshness verdict",
                    }
                ],
            )
        return (name, cands, warnings)

    blueprint_state = providers_state.get("blueprint") or {}
    overlay_state = providers_state.get("dirtyOverlay") or {}
    skills_state = providers_state.get("skills") or {}
    tasks = [
        ("audit", lambda: _adapter("audit", audit.produce, repo_root, task)),
        ("architect", lambda: _adapter("architect", architect.produce, repo_root, task)),
        ("crypt", lambda: _adapter(
            "crypt", crypt.produce_with_observability, repo_root, task, scope_grant_id, scope_descriptor
        )),
        ("git", lambda: _adapter("git", git_provider.produce, repo_root)),
        ("rules", lambda: _adapter("rules", rules.produce, repo_root, task)),
        ("anchors", lambda: _adapter("anchors", anchors.produce, repo_root, explicit_anchors, task)),
    ]
    if bool(blueprint_state.get("usable")):
        tasks.append(
            (
                "blueprint",
                lambda: _adapter(
                    "blueprint",
                    blueprint.produce_with_observability,
                    repo_root,
                    task,
                    max_tokens,
                    expected_generation=expected_blueprint_generation,
                ),
            )
        )
    if bool(overlay_state.get("usable")):
        tasks.append(
            (
                "live",
                lambda: _adapter(
                    "live",
                    live.produce,
                    repo_root,
                    base_commit=freshness.get("baseCommit"),
                    overlay_digest=freshness.get("overlayDigest"),
                    overlay_entries=verdict.get("overlayEntries") or [],
                    prompt_fast=True,
                ),
            )
        )
    if bool(skills_state.get("usable")):
        # Ninth producer: engine-owned skill snapshot (index only; bodies resolve via `crypt
        # skill-read`). The adapter checks the exact returned generation against `/freshness`.
        tasks.append(
            ("skills", lambda: _adapter("skills", skills.produce, repo_root, task, scope_grant_id))
        )

    # Parallel fan-out under one absolute budget. Slow lanes receive typed terminals and cannot
    # hold the prompt waiter open.
    out: list[tuple[str, list[dict[str, Any]], list[dict[str, Any]]]] = []
    if not bool(blueprint_state.get("usable")):
        out.append(
            (
                "blueprint",
                [],
                [
                    {
                        "provider": "blueprint",
                        "kind": "blueprint_lane_degraded",
                        "severity": "warning",
                        "message": f"blueprint lane unavailable: {freshness['graphState']}",
                    }
                ],
            )
        )
    if not bool(skills_state.get("usable")):
        out.append(
            (
                "skills",
                [],
                [
                    {
                        "provider": "skills",
                        "kind": "skills_lane_degraded",
                        "severity": "warning",
                        "message": "skills lane unavailable: generation not coherent",
                    }
                ],
            )
        )
    fanout_started = time.monotonic()
    out.extend(
        _collect_tasks_bounded(tasks, timeout_s=_fanout_timeout_seconds())
    )
    freshness["_stageElapsedMs"] = {
        "freshness": freshness_elapsed_ms,
        **central_stage_elapsed_ms,
        "provider_fanout": max(0.0, (time.monotonic() - fanout_started) * 1000.0),
    }

    order = {name: index for index, name in enumerate(
        ("blueprint", "audit", "architect", "crypt", "git", "live", "rules", "anchors", "skills")
    )}
    out.sort(key=lambda item: order.get(item[0], len(order)))
    freshness["_providerElapsedMs"] = {
        name: provider_elapsed_ms.get(name, 0.0)
        for name in order
    }
    freshness["_providerStageElapsedMs"] = provider_stage_elapsed_ms
    return out, freshness


# ---------------------------------------------------------------------------
# CCS assembly.
# ---------------------------------------------------------------------------


def _merge_candidates(
    per_provider_candidates: list[tuple[str, list[dict[str, Any]], list[dict[str, Any]]]],
    freshness: dict[str, Any],
    repo_root: Path,
    task: str,
    trace_id: str,
    max_tokens: int,
) -> dict[str, Any]:
    """Combine per-provider candidate lists into one ContextCandidateSet v1.

    Dedup by candidate.id (first wins). Provider warnings become
    ProviderWarning omissions (reason=provider_failure, kind=provider),
    surfaced on the planner envelope. The planner produces a typed
    ContextReceipt v2 per warning so the client sees the failure
    attributably. Provider failures NEVER abort the federation.
    """
    seen_ids: set[str] = set()
    merged: list[dict[str, Any]] = []
    provider_warnings: list[dict[str, Any]] = []
    provider_counts: dict[str, int] = {}
    provider_freshness = freshness.get("_providerFreshness") or {}
    provider_base_commit = dict(freshness.get("_providerBaseCommit") or {})
    if freshness.get("blueprintBaseCommit"):
        provider_base_commit.setdefault("blueprint", freshness["blueprintBaseCommit"])
    graph_state = str(freshness.get("graphState") or "indeterminate")

    for provider_name, candidates, warnings in per_provider_candidates:
        provider_counts[provider_name] = len(candidates)
        provider_warnings.extend(
            _normalize_provider_warning(provider_name, warning) for warning in warnings
        )
        for c in candidates:
            cid = c.get("id")
            if not cid or cid in seen_ids:
                continue
            seen_ids.add(cid)
            stamped = dict(c)
            stamped["provider"] = provider_name
            if not stamped.get("freshnessClass"):
                explicit_provider_class = provider_freshness.get(provider_name)
                if explicit_provider_class:
                    stamped["freshnessClass"] = explicit_provider_class
                elif provider_name == "blueprint":
                    stamped["freshnessClass"] = (
                        "committed_snapshot" if graph_state == "clean" else "stale_snapshot"
                    )
                elif provider_name == "live":
                    stamped["freshnessClass"] = "dirty_overlay"
                else:
                    stamped["freshnessClass"] = "current"
            for field in ("baseCommit", "overlayDigest", "snapshotId"):
                value = (
                    provider_base_commit.get(provider_name)
                    if field == "baseCommit"
                    else freshness.get(field)
                )
                if field == "baseCommit" and not value:
                    value = freshness.get(field)
                if value and not stamped.get(field):
                    stamped[field] = value
            merged.append(stamped)

    # Provider failures are recorded in the canonical CCS `omissions`
    # field — the planner sees them and emits a typed receipt per
    # warning. This is the dispatch §G3B "single provider failure must
    # degrade loudly" guarantee at the contract level.
    omissions: list[dict[str, Any]] = []
    for w in provider_warnings:
        omissions.append(
            {
                "id": f"provider_warning:{w['provider']}",
                "reason": w.get("message", "provider_failure")[:200],
                "layer": 0,
                "kind": w.get("kind", "provider_failure"),
                "severity": w.get("severity", "warning"),
            }
        )

    provider_tag = "federated"
    freshness_payload = {
        "revision": trace_id,
        "indexedAt": freshness["indexedAt"],
        "stale": bool(freshness.get("stale", False)),
    }
    for field in (
        "graphState",
        "snapshotId",
        "baseCommit",
        "overlayDigest",
        "expectedReleaseGeneration",
        "observedReleaseGeneration",
        "releaseGenerationStatus",
        "cacheAgeMs",
    ):
        value = freshness.get(field)
        if value is not None:
            freshness_payload[field] = value
    freshness_payload["refreshInFlight"] = bool(
        freshness.get("refreshInFlight", False)
    )
    return {
        "schemaVersion": 1,
        "traceId": trace_id,
        "task": task,
        "mode": "verify",
        "provider": provider_tag,
        "freshness": freshness_payload,
        "providerCeiling": {"maxCandidates": 256, "maxEstimatedTokens": max_tokens},
        "candidates": merged,
        "omissions": omissions,
        "_rightcontext": {
            "providerCounts": provider_counts,
            "providerWarnings": provider_warnings,
            "providerElapsedMs": dict(freshness.get("_providerElapsedMs") or {}),
            "providerStageElapsedMs": dict(freshness.get("_providerStageElapsedMs") or {}),
            "serviceGeneration": freshness.get("serviceGeneration") or "unknown",
            "expectedReleaseGeneration": freshness.get("expectedReleaseGeneration"),
            "observedReleaseGeneration": freshness.get("observedReleaseGeneration"),
            "releaseGenerationStatus": freshness.get("releaseGenerationStatus"),
            "firstAfterIdle": bool(freshness.get("firstAfterIdle", False)),
            "idleGapMs": freshness.get("idleGapMs"),
            "cacheAgeMs": freshness.get("cacheAgeMs"),
            "refreshInFlight": bool(freshness.get("refreshInFlight", False)),
            "stageElapsedMs": dict(freshness.get("_stageElapsedMs") or {}),
            "repoRoot": str(repo_root),
        },
    }


def assemble_candidate_set(
    *,
    task: str,
    repo: str,
    max_tokens: int = 4096,
    client: str = "claude",
    session: str | None = None,
    anchors: str = "",
    scope_grant_id: str | None = None,
    scope_descriptor: dict | None = None,
) -> tuple[dict, int]:
    """One federation assembly: (envelope, exit_code). Shared by the argv
    one-shot mode and the resident stdio worker; behavior identical."""
    repo_root = _resolve_repo_root(repo)
    explicit_anchors = [a.strip() for a in (anchors or "").split(",") if a.strip()]
    trace_id = _trace_id()

    # ScopeGrant enforcement happens BEFORE any provider fan-out (fail-closed).
    try:
        _enforce_scope_grant(
            repo_root, scope_grant_id, client=client, task=task, session=session
        )
    except PermissionError as exc:
        err_envelope = {
            "schemaVersion": 1,
            "traceId": trace_id,
            "task": task,
            "mode": "verify",
            "provider": "federated",
            "freshness": {
                "revision": trace_id,
                "indexedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "stale": False,
            },
            "providerCeiling": {"maxCandidates": 256, "maxEstimatedTokens": max_tokens},
            "candidates": [],
            "omissions": [
                {
                    "id": f"scope_grant_abort:{scope_grant_id}",
                    "reason": str(exc)[:200],
                    "layer": 0,
                    "kind": "scope_grant_aborted",
                    "severity": "blocker",
                }
            ],
            "_rightcontext": {
                "abortReason": "scope_grant",
                "abortDetail": str(exc)[:400],
                "providerCounts": {},
                "providerWarnings": [],
                "repoRoot": str(repo_root),
            },
        }
        sys.stderr.write("federation_aborted scope_grant: " + str(exc)[:200] + chr(10))
        return err_envelope, 2

    per_provider, freshness = _gather_all_parallel(
        task=task,
        repo_root=repo_root,
        max_tokens=max_tokens,
        explicit_anchors=explicit_anchors,
        scope_grant_id=scope_grant_id,
        scope_descriptor=scope_descriptor,
    )
    merge_started = time.monotonic()
    ccs = _merge_candidates(
        per_provider, freshness, repo_root, task, trace_id, max_tokens
    )
    ccs["_rightcontext"]["stageElapsedMs"]["merge"] = max(
        0.0, (time.monotonic() - merge_started) * 1000.0
    )
    return ccs, 0


def _gateway_source_sha256() -> str:
    try:
        return hashlib.sha256(Path(__file__).resolve().read_bytes()).hexdigest()
    except OSError:
        return "unknown"


def serve_stdio() -> int:
    """Persistent-worker mode (Bazel WorkRequest shape): one JSON request per
    stdin line, one CCS JSON per stdout line.

    - readline(), never `for line in sys.stdin`: the iterator's read-ahead
      buffer can hold a delivered line back (the classic LSP/worker stall,
      worst on Windows pipes).
    - First stdout line is the readiness handshake carrying this file's
      sha256; the supervisor gates requests on it and restarts on mismatch.
    - A bad request yields an abort envelope; the worker itself survives.
    - Providers are imported at module load, so the fan-out is warm from the
      first request.
    """
    sys.stdout.write(
        json.dumps(
            {"workerReady": {"gatewaySha256": _gateway_source_sha256(), "pid": os.getpid()}}
        )
        + "\n"
    )
    sys.stdout.flush()
    while True:
        line = sys.stdin.readline()
        if not line:  # EOF — supervisor closed the pipe
            return 0
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            envelope, _code = assemble_candidate_set(
                task=str(req.get("task") or "(empty)"),
                repo=str(req["repo"]),
                max_tokens=int(req.get("maxTokens") or 4096),
                client=str(req.get("client") or "claude"),
                session=req.get("session"),
                anchors=str(req.get("anchors") or ""),
                scope_grant_id=req.get("scopeGrantId"),
                scope_descriptor=req.get("scopeDescriptor"),
            )
            payload = json.dumps(envelope, ensure_ascii=False)
        except Exception as exc:  # noqa: BLE001 — a bad request must not kill the worker
            payload = json.dumps(
                {
                    "_rightcontext": {
                        "abortReason": "worker_request_failed",
                        "abortDetail": str(exc)[:200],
                    }
                }
            )
        sys.stdout.write(payload + "\n")
        sys.stdout.flush()


def main(argv: list[str] | None = None) -> int:
    raw_argv = sys.argv[1:] if argv is None else argv
    if "--serve-stdio" in raw_argv:
        return serve_stdio()
    parser = argparse.ArgumentParser(description="RightContext federation gateway")
    parser.add_argument("--task", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--max-tokens", type=int, default=4096)
    parser.add_argument("--client", default="claude")
    parser.add_argument("--session", default=None)
    parser.add_argument("--anchors", default="",
                        help="Comma-separated explicit anchor list")
    parser.add_argument("--scope-grant-id", default=None)
    parser.add_argument("--scope-descriptor-json", default=None)
    args = parser.parse_args(argv)

    try:
        scope_descriptor = (
            json.loads(args.scope_descriptor_json)
            if args.scope_descriptor_json is not None
            else None
        )
    except json.JSONDecodeError as exc:
        raise SystemExit("scope descriptor must be JSON") from exc
    if scope_descriptor is not None and not isinstance(scope_descriptor, dict):
        raise SystemExit("scope descriptor must be a JSON object")

    envelope, code = assemble_candidate_set(
        task=args.task,
        repo=args.repo,
        max_tokens=args.max_tokens,
        client=args.client,
        session=args.session,
        anchors=args.anchors,
        scope_grant_id=args.scope_grant_id,
        scope_descriptor=scope_descriptor,
    )
    # Emit the assembled envelope with the `_rightcontext` federation
    # detail block intact. The planner strict-deserializes the canonical
    # CCS fields only; the dispatcher surfaces the warnings and provider
    # counts to the client envelope.
    print(json.dumps(envelope))
    return code


if __name__ == "__main__":
    sys.exit(main())
