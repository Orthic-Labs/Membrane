#!/usr/bin/env python3
"""Issue and validate content-free Adapt multi-installation conformance receipts.

The receipt binds one schema-v2 installation to the current canonical Adapt
pool, exact implementation and test files, the installed resident binary and
its authenticated atomic-batch route, local transcript discovery counts, the
append-only Git mirror boundary, and a disabled ``crypt-daily`` scheduler.
It never starts, stops, installs, or schedules anything.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Iterable, Mapping, Sequence


ADAPT_DIR = Path(__file__).resolve().parent
import workspace_runtime  # noqa: E402

REPO_ROOT = workspace_runtime.workspace_root()
TOOLS_LIB = REPO_ROOT / "tools" / "lib"
MEMORY_DIR = REPO_ROOT / "tools" / "pipelines" / "memory"
for directory in (REPO_ROOT, ADAPT_DIR, TOOLS_LIB, MEMORY_DIR):
    if str(directory) not in sys.path:
        sys.path.insert(0, str(directory))

import cross_machine  # noqa: E402
import taste_runtime  # noqa: E402
import taste_v2_pipeline  # noqa: E402
import transcript_sources  # noqa: E402

mirror_append_only = workspace_runtime.mirror_append_only()
crypt_port = workspace_runtime.crypt_port


SCHEMA_VERSION = 1
KIND = "adapt_multiwriter_conformance"
MAX_RECEIPT_TTL_SECONDS = 3600
DEFAULT_TTL_SECONDS = 900
SHA256_RE = re.compile(r"^[a-f0-9]{64}$")
GENERATION_RE = re.compile(r"^sha256:[a-f0-9]{64}$")
GIT_OID_RE = re.compile(r"^[a-f0-9]{40}(?:[a-f0-9]{24})?$")
CLIENT_RE = re.compile(r"^[a-z0-9][a-z0-9._:-]{0,79}$")
DISCOVERY_OUTCOMES = ("parsed", "unreadable", "malformed", "skipped")
DISCOVERY_FIELDS = ("discovered", *DISCOVERY_OUTCOMES, "pending")

DEFAULT_IMPLEMENTATION_FILES = (
    "tools/pipelines/memory/context_session_adapters.py",
    "tools/pipelines/memory/context_session_inventory.py",
    "adapt/taste_apply.py",
    "adapt/taste_runtime.py",
    "adapt/taste_v2_pipeline.py",
    "adapt/transcript_sources.py",
    "adapt/adapt_persistence.py",
    "adapt/cross_machine.py",
    "adapt/adjudicate_manifest.py",
    "adapt/manifest.py",
    "adapt/preference_record.py",
    "adapt/preference-manifest.schema.json",
    "adapt/multiwriter_conformance.py",
    "adapt/run_incremental_multiwriter.py",
    "adapt/adapt_event_learning.py",
)
DEFAULT_TEST_FILES = (
    "tools/pipelines/memory/test_context_session_harnesses.py",
    "adapt/test_multiwriter_conformance.py",
    "adapt/test_run_incremental_multiwriter.py",
    "adapt/test_adapt_event_learning.py",
)
EVIDENCE_KEYS = {
    "installation_id",
    "canonical_pool",
    "implementation",
    "installed_service",
    "discovery",
    "source_clients",
    "mirror",
    "scheduler",
    "focused_tests",
}
RECEIPT_KEYS = EVIDENCE_KEYS | {
    "schema_version",
    "kind",
    "issued_at",
    "expires_at",
    "receipt_sha256",
}


class ConformanceError(RuntimeError):
    """Raised when an installation cannot safely run multiwriter Adapt."""


def _canonical(value: object) -> bytes:
    try:
        return json.dumps(
            value,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
            allow_nan=False,
        ).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise ConformanceError("conformance evidence is not canonical JSON") from exc


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with Path(path).open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as exc:
        raise ConformanceError(f"required conformance file is unavailable: {path}") from exc
    return digest.hexdigest()


def _utc(value: datetime) -> datetime:
    if value.tzinfo is None:
        raise ConformanceError("conformance time must be timezone-aware")
    return value.astimezone(timezone.utc).replace(microsecond=0)


def _stamp(value: datetime) -> str:
    return _utc(value).isoformat().replace("+00:00", "Z")


def _parse_stamp(value: object, label: str) -> datetime:
    try:
        parsed = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    except (TypeError, ValueError) as exc:
        raise ConformanceError(f"invalid receipt {label}") from exc
    if parsed.tzinfo is None or _stamp(parsed) != value:
        raise ConformanceError(f"invalid receipt {label}")
    return parsed.astimezone(timezone.utc)


def _uuid4(value: object) -> str:
    try:
        parsed = uuid.UUID(str(value))
    except (TypeError, ValueError) as exc:
        raise ConformanceError("invalid schema-v2 installation UUID") from exc
    if parsed.version != 4 or str(parsed) != value:
        raise ConformanceError("invalid schema-v2 installation UUID")
    return str(parsed)


def aggregate_file_sha256(files: Sequence[Mapping[str, str]]) -> str:
    rows = [{"path": row["path"], "sha256": row["sha256"]} for row in files]
    return _sha256_bytes(_canonical(rows))


def hash_sources(repo_root: Path, paths: Iterable[Path | str]) -> dict[str, Any]:
    root = Path(repo_root).resolve()
    rows = []
    seen = set()
    for value in paths:
        path = Path(value)
        if not path.is_absolute():
            path = root / path
        try:
            relative = path.resolve().relative_to(root).as_posix()
        except (OSError, ValueError) as exc:
            raise ConformanceError(f"implementation source escapes repository: {path}") from exc
        if relative in seen:
            raise ConformanceError(f"duplicate implementation source: {relative}")
        seen.add(relative)
        rows.append({"path": relative, "sha256": _sha256_file(path)})
    if not rows:
        raise ConformanceError("implementation source set is empty")
    rows.sort(key=lambda row: row["path"])
    return {"files": rows, "aggregate_sha256": aggregate_file_sha256(rows)}


def discovery_counts(
    discovered: Iterable[Mapping[str, Any]],
    state: Mapping[str, Any],
    *,
    registered_clients: Iterable[str] = (),
) -> dict[str, dict[str, int]]:
    clients = {str(client) for client in registered_clients}
    if any(not CLIENT_RE.fullmatch(client) for client in clients):
        raise ConformanceError("session adapter registered an invalid client")
    counts = {
        client: {field: 0 for field in DISCOVERY_FIELDS}
        for client in sorted(clients)
    }
    learned = state.get("learned", {}) if isinstance(state, Mapping) else {}
    for item in discovered:
        if not isinstance(item, Mapping):
            raise ConformanceError("session adapter emitted invalid accounting")
        client = str(item.get("client") or "")
        tool = str(item.get("tool") or "")
        outcome = str(item.get("outcome") or "")
        if not CLIENT_RE.fullmatch(client) or not CLIENT_RE.fullmatch(tool):
            raise ConformanceError("session adapter emitted invalid accounting")
        if outcome not in DISCOVERY_OUTCOMES:
            raise ConformanceError("session adapter emitted invalid accounting")
        path = Path(item.get("path"))
        counts.setdefault(client, {field: 0 for field in DISCOVERY_FIELDS})
        try:
            mtime = path.stat().st_mtime
        except OSError as exc:
            raise ConformanceError("transcript discovery changed during conformance") from exc
        counts[client]["discovered"] += 1
        counts[client][outcome] += 1
        # Taste v2 owns one canonical learned map keyed by source-local key;
        # legacy per-tool session state must never influence conformance.
        state_key = str(item.get("state_key") or path.stem)
        if (
            outcome == "parsed"
            and (not isinstance(learned, Mapping) or state_key not in learned)
        ):
            counts[client]["pending"] += 1
    return {client: counts[client] for client in sorted(counts)}


def discover_session_evidence(
    *,
    state: Mapping[str, Any] | None = None,
    installation_id: str | None = None,
    env: Mapping[str, str] = os.environ,
) -> dict[str, Any]:
    """Return content-free Taste v2 discovery, quarantine & source-client join."""
    del env  # Discovery owns its environment boundary through transcript_sources.
    effective_state = state if state is not None else taste_v2_pipeline.load_state(
        taste_runtime.state_path()
    )
    learned = effective_state.get("learned", {}) if isinstance(effective_state, Mapping) else {}
    discovered = transcript_sources.discover()
    selected, quarantined = transcript_sources.select_sources(
        discovered, learned=learned if isinstance(learned, dict) else {}
    )
    rows: list[dict[str, Any]] = []
    source_clients: dict[str, str] = {}
    clients: set[str] = set()
    for source in [*selected, *quarantined]:
        host = source.spec.host
        client = "claude" if host == "claude_code" else "codex" if host == "codex" else source.spec.tool
        outcome = "parsed" if source in selected else "skipped"
        clients.add(client)
        rows.append({
            "client": client,
            "tool": source.spec.tool,
            "path": source.path,
            "outcome": outcome,
            "state_key": source.local_source_key,
        })
        if outcome == "parsed" and installation_id is not None:
            source_id = cross_machine.qualify_source_session(
                installation_id,
                source.spec.tool,
                source.session_id,
            )
            existing = source_clients.setdefault(source_id, client)
            if existing != client:
                raise ConformanceError("session source maps to multiple clients")
    return {
        "discovery": discovery_counts(
            rows,
            effective_state,
            registered_clients=clients,
        ),
        "source_clients": source_clients,
    }


def source_client_rows(source_clients: Mapping[str, str]) -> list[dict[str, str]]:
    rows = [
        {
            "source_sha256": _sha256_bytes(str(source_id).encode("utf-8")),
            "client": str(client),
        }
        for source_id, client in source_clients.items()
    ]
    if any(not CLIENT_RE.fullmatch(row["client"]) for row in rows):
        raise ConformanceError("session source maps to an invalid client")
    rows.sort(key=lambda row: (row["source_sha256"], row["client"]))
    if len({row["source_sha256"] for row in rows}) != len(rows):
        raise ConformanceError("session source digest maps to multiple clients")
    return rows


def _normalized_os() -> str:
    value = platform.system()
    if value == "Windows":
        return "windows"
    if value == "Darwin":
        return "macos"
    raise ConformanceError(f"unsupported Adapt conformance platform: {value}")


def _normalized_arch() -> str:
    value = platform.machine().lower()
    if value in {"amd64", "x86_64"}:
        return "x86_64"
    if value in {"arm64", "aarch64"}:
        return "aarch64"
    raise ConformanceError(f"unsupported Adapt conformance architecture: {value}")


def service_evidence(
    *,
    binary_path: Path,
    release_manifest_path: Path,
    os_name: str,
    arch: str,
    probe: Callable[[], Mapping[str, Any]],
) -> dict[str, Any]:
    binary = Path(binary_path)
    binary_sha = _sha256_file(binary)
    try:
        release_bytes = Path(release_manifest_path).read_bytes()
        release = json.loads(release_bytes)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ConformanceError("Crypt release manifest is unavailable or invalid") from exc
    if not isinstance(release, dict):
        raise ConformanceError("Crypt release manifest is unavailable or invalid")
    role = "service" if binary.name.startswith("crypt-service") else "cli"
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise ConformanceError("Crypt release manifest has no assets")
    matches = [
        row
        for row in assets
        if isinstance(row, dict)
        and row.get("os") == os_name
        and row.get("arch") == arch
        and row.get("role") == role
        and row.get("name") == binary.name
    ]
    if len(matches) != 1 or matches[0].get("sha256") != binary_sha:
        raise ConformanceError("installed binary does not match its release asset")
    release_generation = release.get("release_generation")
    if not isinstance(release_generation, str) or not GENERATION_RE.fullmatch(release_generation):
        raise ConformanceError("release manifest generation is invalid")
    observed = dict(probe())
    if observed.get("release_generation") != release_generation:
        raise ConformanceError("resident service release does not match installed asset")
    service_generation = observed.get("service_generation")
    if not isinstance(service_generation, str) or not GENERATION_RE.fullmatch(service_generation):
        raise ConformanceError("resident service generation is invalid")
    route_capable = (
        observed.get("batch_route_status") == 400
        and observed.get("batch_route_error") == "invalid memory batch"
    )
    if not route_capable:
        raise ConformanceError("authenticated atomic memory batch route is unavailable")
    return {
        "binary_role": role,
        "binary_sha256": binary_sha,
        "release_manifest_sha256": _sha256_bytes(release_bytes),
        "release_generation": release_generation,
        "service_generation": service_generation,
        "batch_route_status": 400,
        "batch_route_capable": True,
    }


def _read_json_response(request: urllib.request.Request, timeout: float) -> tuple[int, dict]:
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
            status = response.status
    except urllib.error.HTTPError as exc:
        status = exc.code
        raw = exc.read()
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise ConformanceError("resident Crypt service is unavailable") from exc
    try:
        payload = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ConformanceError("resident Crypt response is not valid JSON") from exc
    if not isinstance(payload, dict):
        raise ConformanceError("resident Crypt response is not an object")
    return status, payload


def probe_resident_service(
    *, service_url: str, token_file: Path, timeout: float = 5.0
) -> dict[str, Any]:
    parsed = urllib.parse.urlsplit(service_url)
    if parsed.scheme != "http" or parsed.hostname not in {"127.0.0.1", "localhost"}:
        raise ConformanceError("Crypt conformance probe must remain loopback-only")
    base = service_url.rstrip("/")
    health_request = urllib.request.Request(
        base + "/health", headers={"Accept": "application/json"}, method="GET"
    )
    health_status, health = _read_json_response(health_request, timeout)
    if health_status != 200 or health.get("ok") is not True:
        raise ConformanceError("resident Crypt health is not green")
    try:
        token = Path(token_file).read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise ConformanceError("Crypt API token is unavailable") from exc
    if not token:
        raise ConformanceError("Crypt API token is empty")
    route_request = urllib.request.Request(
        base + "/v1/memories:batch",
        data=b"{}",
        headers={
            "Accept": "application/json",
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    route_status, route = _read_json_response(route_request, timeout)
    return {
        "release_generation": health.get("releaseGeneration"),
        "service_generation": health.get("serviceGeneration"),
        "batch_route_status": route_status,
        "batch_route_error": route.get("error"),
    }


def _hidden_creation_flags() -> int:
    return int(getattr(subprocess, "CREATE_NO_WINDOW", 0)) if os.name == "nt" else 0


def _scheduler_command(argv: Sequence[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(argv),
        shell=False,
        capture_output=True,
        text=True,
        timeout=15,
        creationflags=_hidden_creation_flags(),
    )


def scheduler_evidence(
    *,
    system: str | None = None,
    command_runner: Callable[[Sequence[str]], subprocess.CompletedProcess[str]] = _scheduler_command,
) -> dict[str, Any]:
    current = system or platform.system()
    if current == "Windows":
        script = (
            "$t=Get-ScheduledTask -TaskName 'crypt-daily' -ErrorAction SilentlyContinue "
            "| Select-Object -First 1; if($null -eq $t){'absent'}else{$t.State.ToString().ToLowerInvariant()}"
        )
        result = command_runner([
            "powershell.exe",
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        if result.returncode != 0:
            raise ConformanceError("cannot inspect crypt-daily scheduler")
        state = result.stdout.strip().lower()
        disabled = state in {"absent", "disabled"}
    elif current == "Darwin":
        uid = str(os.getuid())
        result = command_runner([
            "launchctl", "print", f"gui/{uid}/com.adrian.crypt-daily"
        ])
        disabled = result.returncode != 0
        state = "unloaded" if disabled else "loaded"
    else:
        raise ConformanceError(f"unsupported Adapt scheduler platform: {current}")
    if not disabled:
        raise ConformanceError("crypt-daily must remain disabled")
    return {"name": "crypt-daily", "disabled": True, "state": state}


def _git(repo: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=repo,
            shell=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        raise ConformanceError("Git mirror inspection failed") from exc
    if result.returncode != 0:
        raise ConformanceError("Git mirror inspection failed")
    return result.stdout.strip()


def mirror_evidence(
    *, repo_root: Path, mirror_relative: Path, installation_file: Path
) -> dict[str, Any]:
    repo = Path(repo_root).resolve()
    mirror_relative = Path(mirror_relative)
    try:
        installation_id = cross_machine.load_installation_id(Path(installation_file))
        origin = mirror_append_only.replica_key(installation_id)
        mirror_append_only.validate_git_worktree(
            repo,
            mirror_relative=mirror_relative,
            owner_origin=origin,
            owner_cursor=f"{origin}.json",
            owner_installation_id=installation_id,
        )
    except (cross_machine.CrossMachineAdaptError, mirror_append_only.AppendOnlyViolation) as exc:
        raise ConformanceError("memory mirror append-only check failed") from exc
    head = _git(repo, "rev-parse", "HEAD")
    tree_oid = _git(repo, "rev-parse", f"HEAD:{mirror_relative.as_posix()}")
    if not GIT_OID_RE.fullmatch(head) or not GIT_OID_RE.fullmatch(tree_oid):
        raise ConformanceError("memory mirror Git identity is invalid")
    events_root = repo / mirror_relative / "_events"
    events = sum(1 for path in events_root.glob("*/*.json") if path.is_file())
    return {
        "append_only": True,
        "head": head,
        "tree_oid": tree_oid,
        "events": events,
    }


def run_focused_tests(
    repo_root: Path,
    test_files: Sequence[Path | str] = DEFAULT_TEST_FILES,
) -> dict[str, Any]:
    root = Path(repo_root).resolve()
    hashed = hash_sources(root, test_files)
    managed_python = root / ".venv-tools" / (
        "Scripts/python.exe" if os.name == "nt" else "bin/python"
    )
    test_python = Path(sys.executable)
    if managed_python.is_file():
        probe = subprocess.run(
            [str(managed_python), "-c", "import pytest"],
            capture_output=True,
            text=True,
            timeout=10,
            creationflags=_hidden_creation_flags(),
        )
        if probe.returncode == 0:
            test_python = managed_python
    argv = [str(test_python), "-m", "pytest", *[row["path"] for row in hashed["files"]], "-q"]
    try:
        result = subprocess.run(
            argv,
            cwd=root,
            shell=False,
            capture_output=True,
            text=True,
            timeout=180,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        raise ConformanceError("focused conformance tests could not run") from exc
    combined = (result.stdout + "\n" + result.stderr).encode("utf-8")
    matches = re.findall(r"(?<![\w])([0-9]+) passed", result.stdout)
    passed_count = int(matches[-1]) if matches else 0
    if result.returncode != 0 or passed_count <= 0:
        raise ConformanceError("focused tests for conformance are not green")
    return {
        "passed": True,
        "passed_count": passed_count,
        "output_sha256": _sha256_bytes(combined),
        "files": hashed["files"],
    }


def _validated_prior_tests(repo_root: Path, prior: Mapping[str, Any]) -> dict[str, Any]:
    if prior.get("passed") is not True:
        raise ConformanceError("focused tests for conformance are not green")
    files = prior.get("files")
    if not isinstance(files, list) or not files:
        raise ConformanceError("focused conformance test evidence is invalid")
    current = hash_sources(Path(repo_root), [row.get("path", "") for row in files])
    if current["files"] != files:
        raise ConformanceError("focused conformance test sources changed")
    passed_count = prior.get("passed_count")
    output_sha = prior.get("output_sha256")
    if (
        isinstance(passed_count, bool)
        or not isinstance(passed_count, int)
        or passed_count <= 0
        or not isinstance(output_sha, str)
        or not SHA256_RE.fullmatch(output_sha)
    ):
        raise ConformanceError("focused conformance test evidence is invalid")
    return copy.deepcopy(dict(prior))


def collect_evidence(
    *,
    repo_root: Path,
    installation_file: Path,
    db_path: Path,
    binary_path: Path,
    release_manifest_path: Path,
    mirror_relative: Path = Path("memory-mirror"),
    service_url: str | None = None,
    token_file: Path | None = None,
    focused_tests: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    root = Path(repo_root).resolve()
    installation_id = cross_machine.load_installation_id(Path(installation_file))
    canonical_rules = cross_machine.load_canonical_rules(Path(db_path))
    state = taste_v2_pipeline.load_state(taste_runtime.state_path())
    tests = (
        run_focused_tests(root)
        if focused_tests is None
        else _validated_prior_tests(root, focused_tests)
    )
    url = service_url or f"http://127.0.0.1:{crypt_port(os.environ)}"
    token = Path(token_file) if token_file else Path(db_path).parent / "api-token"
    sessions = discover_session_evidence(
        state=state,
        installation_id=installation_id,
    )
    return {
        "installation_id": installation_id,
        "canonical_pool": {
            "sha256": cross_machine.canonical_pool_sha256(canonical_rules),
            "rules": len(canonical_rules),
        },
        "implementation": hash_sources(root, DEFAULT_IMPLEMENTATION_FILES),
        "installed_service": service_evidence(
            binary_path=Path(binary_path),
            release_manifest_path=Path(release_manifest_path),
            os_name=_normalized_os(),
            arch=_normalized_arch(),
            probe=lambda: probe_resident_service(
                service_url=url, token_file=token
            ),
        ),
        "discovery": sessions["discovery"],
        "source_clients": source_client_rows(sessions["source_clients"]),
        "mirror": mirror_evidence(
            repo_root=root,
            mirror_relative=mirror_relative,
            installation_file=Path(installation_file),
        ),
        "scheduler": scheduler_evidence(),
        "focused_tests": tests,
    }


def _require_hash(value: object, label: str) -> None:
    if not isinstance(value, str) or not SHA256_RE.fullmatch(value):
        raise ConformanceError(f"invalid {label}")


def _validate_file_rows(value: object, label: str) -> list[Mapping[str, Any]]:
    if not isinstance(value, list) or not value:
        raise ConformanceError(f"{label} files are invalid")
    paths = []
    for row in value:
        if not isinstance(row, Mapping) or set(row) != {"path", "sha256"}:
            raise ConformanceError(f"{label} file fields are invalid")
        path = row.get("path")
        if (
            not isinstance(path, str)
            or not path
            or "\\" in path
            or path.startswith("/")
            or PurePosixPath(path).is_absolute()
            or any(part in {"", ".", ".."} for part in PurePosixPath(path).parts)
        ):
            raise ConformanceError(f"{label} file path is invalid")
        _require_hash(row.get("sha256"), f"{label} file hash")
        paths.append(path)
    if paths != sorted(set(paths)):
        raise ConformanceError(f"{label} files must be unique and sorted")
    return value


def _validate_evidence(evidence: Mapping[str, Any]) -> None:
    if set(evidence) != EVIDENCE_KEYS:
        raise ConformanceError("conformance evidence fields are invalid")
    _uuid4(evidence.get("installation_id"))
    pool = evidence.get("canonical_pool")
    if not isinstance(pool, Mapping) or set(pool) != {"sha256", "rules"}:
        raise ConformanceError("canonical Adapt pool fields are invalid")
    if not isinstance(pool, Mapping):
        raise ConformanceError("canonical Adapt pool evidence is invalid")
    _require_hash(pool.get("sha256"), "canonical Adapt pool hash")
    if isinstance(pool.get("rules"), bool) or not isinstance(pool.get("rules"), int) or pool["rules"] < 0:
        raise ConformanceError("canonical Adapt pool rule count is invalid")
    implementation = evidence.get("implementation")
    if (
        not isinstance(implementation, Mapping)
        or set(implementation) != {"files", "aggregate_sha256"}
    ):
        raise ConformanceError("implementation hash evidence is invalid")
    files = _validate_file_rows(implementation.get("files"), "implementation")
    _require_hash(implementation.get("aggregate_sha256"), "implementation aggregate hash")
    if implementation["aggregate_sha256"] != aggregate_file_sha256(files):
        raise ConformanceError("implementation aggregate hash mismatch")
    service = evidence.get("installed_service")
    if (
        not isinstance(service, Mapping)
        or set(service) != {
            "binary_role",
            "binary_sha256",
            "release_manifest_sha256",
            "release_generation",
            "service_generation",
            "batch_route_status",
            "batch_route_capable",
        }
        or service.get("batch_route_capable") is not True
    ):
        raise ConformanceError("authenticated atomic memory batch route is unavailable")
    if service.get("binary_role") not in {"cli", "service"}:
        raise ConformanceError("installed service binary role is invalid")
    for key in ("binary_sha256", "release_manifest_sha256"):
        _require_hash(service.get(key), f"installed service {key}")
    if service.get("batch_route_status") != 400:
        raise ConformanceError("authenticated atomic memory batch route is unavailable")
    for key in ("release_generation", "service_generation"):
        if not isinstance(service.get(key), str) or not GENERATION_RE.fullmatch(service[key]):
            raise ConformanceError(f"installed service {key} is invalid")
    discovery = evidence.get("discovery")
    if not isinstance(discovery, Mapping) or not discovery:
        raise ConformanceError("transcript discovery evidence is invalid")
    for client, row in discovery.items():
        if not isinstance(client, str) or not CLIENT_RE.fullmatch(client):
            raise ConformanceError("transcript discovery evidence is invalid")
        if not isinstance(row, Mapping) or set(row) != set(DISCOVERY_FIELDS):
            raise ConformanceError("transcript discovery evidence is invalid")
        if any(isinstance(row[key], bool) or not isinstance(row[key], int) or row[key] < 0 for key in row):
            raise ConformanceError("transcript discovery evidence is invalid")
        if row["pending"] > row["discovered"]:
            raise ConformanceError("transcript discovery evidence is invalid")
        if sum(row[key] for key in DISCOVERY_OUTCOMES) != row["discovered"]:
            raise ConformanceError("transcript discovery evidence is invalid")
    source_clients = evidence.get("source_clients")
    if not isinstance(source_clients, list):
        raise ConformanceError("transcript source client evidence is invalid")
    observed_sources: set[str] = set()
    for row in source_clients:
        if not isinstance(row, Mapping) or set(row) != {"source_sha256", "client"}:
            raise ConformanceError("transcript source client evidence is invalid")
        source_sha = row.get("source_sha256")
        client = row.get("client")
        if (
            not isinstance(source_sha, str)
            or not SHA256_RE.fullmatch(source_sha)
            or source_sha in observed_sources
            or not isinstance(client, str)
            or not CLIENT_RE.fullmatch(client)
            or client not in discovery
        ):
            raise ConformanceError("transcript source client evidence is invalid")
        observed_sources.add(source_sha)
    if source_clients != sorted(
        source_clients,
        key=lambda row: (row["source_sha256"], row["client"]),
    ):
        raise ConformanceError("transcript source client evidence is invalid")
    mirror = evidence.get("mirror")
    if (
        not isinstance(mirror, Mapping)
        or set(mirror) != {"append_only", "head", "tree_oid", "events"}
        or mirror.get("append_only") is not True
    ):
        raise ConformanceError("memory mirror append-only check is not green")
    if (
        not isinstance(mirror.get("head"), str)
        or not GIT_OID_RE.fullmatch(mirror["head"])
        or not isinstance(mirror.get("tree_oid"), str)
        or not GIT_OID_RE.fullmatch(mirror["tree_oid"])
        or isinstance(mirror.get("events"), bool)
        or not isinstance(mirror.get("events"), int)
        or mirror["events"] < 0
    ):
        raise ConformanceError("memory mirror evidence is invalid")
    scheduler = evidence.get("scheduler")
    if (
        not isinstance(scheduler, Mapping)
        or set(scheduler) != {"name", "disabled", "state"}
        or scheduler.get("name") != "crypt-daily"
        or scheduler.get("disabled") is not True
        or scheduler.get("state") not in {"absent", "disabled", "unloaded"}
    ):
        raise ConformanceError("crypt-daily must remain disabled")
    tests = evidence.get("focused_tests")
    if (
        not isinstance(tests, Mapping)
        or set(tests) != {"passed", "passed_count", "output_sha256", "files"}
        or tests.get("passed") is not True
    ):
        raise ConformanceError("focused tests for conformance are not green")
    if (
        isinstance(tests.get("passed_count"), bool)
        or not isinstance(tests.get("passed_count"), int)
        or tests["passed_count"] <= 0
    ):
        raise ConformanceError("focused test count is invalid")
    _require_hash(tests.get("output_sha256"), "focused test output hash")
    _validate_file_rows(tests.get("files"), "focused test")


def receipt_sha256(receipt: Mapping[str, Any]) -> str:
    unsigned = {key: value for key, value in receipt.items() if key != "receipt_sha256"}
    return _sha256_bytes(_canonical(unsigned))


def issue_receipt(
    evidence: Mapping[str, Any], *, now: datetime, ttl_seconds: int = DEFAULT_TTL_SECONDS
) -> dict[str, Any]:
    _validate_evidence(evidence)
    if isinstance(ttl_seconds, bool) or not 1 <= ttl_seconds <= MAX_RECEIPT_TTL_SECONDS:
        raise ConformanceError("receipt freshness window is invalid")
    issued = _utc(now)
    receipt = {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "issued_at": _stamp(issued),
        "expires_at": _stamp(issued + timedelta(seconds=ttl_seconds)),
        **copy.deepcopy(dict(evidence)),
    }
    receipt["receipt_sha256"] = receipt_sha256(receipt)
    return receipt


def validate_receipt_payload(
    receipt: Mapping[str, Any],
    *,
    current_evidence: Mapping[str, Any],
    now: datetime,
    allow_session_discovery_drift: bool = False,
) -> dict[str, Any]:
    if set(receipt) != RECEIPT_KEYS:
        raise ConformanceError("receipt fields are invalid")
    if receipt.get("schema_version") != SCHEMA_VERSION or receipt.get("kind") != KIND:
        raise ConformanceError("receipt identity is invalid")
    expected_hash = receipt_sha256(receipt)
    if receipt.get("receipt_sha256") != expected_hash:
        raise ConformanceError("receipt hash mismatch")
    issued = _parse_stamp(receipt.get("issued_at"), "issued_at")
    expires = _parse_stamp(receipt.get("expires_at"), "expires_at")
    current = _utc(now)
    if expires <= issued or (expires - issued).total_seconds() > MAX_RECEIPT_TTL_SECONDS:
        raise ConformanceError("receipt freshness window is invalid")
    if current < issued:
        raise ConformanceError("receipt is not yet valid")
    if current > expires:
        raise ConformanceError("receipt expired")
    embedded = {key: receipt[key] for key in EVIDENCE_KEYS}
    _validate_evidence(embedded)
    _validate_evidence(current_evidence)
    compared_embedded = embedded
    compared_current = current_evidence
    if allow_session_discovery_drift:
        volatile = {"discovery", "source_clients"}
        compared_embedded = {
            key: value for key, value in embedded.items() if key not in volatile
        }
        compared_current = {
            key: value for key, value in current_evidence.items() if key not in volatile
        }
    if _canonical(compared_embedded) != _canonical(compared_current):
        raise ConformanceError("receipt does not match current conformance evidence")
    return copy.deepcopy(dict(receipt))


def _defaults(repo_root: Path) -> dict[str, Path]:
    root = Path(repo_root).resolve()
    os_name = _normalized_os()
    binary_name = "crypt-service.exe" if os_name == "windows" else "crypt"
    db = Path(os.environ.get(
        "CRYPT_DB", str(root / "tools/.cache/memory/crypt-engine.db")
    ))
    return {
        "installation_file": root / "tools/.cache/memory/installation.json",
        "db_path": db,
        "binary_path": Path(os.environ.get(
            "CRYPT_CONFORMANCE_BINARY", str(root / "tools/bin" / binary_name)
        )),
        "release_manifest_path": Path(os.environ.get(
            "CRYPT_CONFORMANCE_RELEASE_MANIFEST",
            str(root / "tools/lib/crypt-release.json"),
        )),
        "token_file": Path(os.environ.get(
            "CRYPT_API_TOKEN_FILE", str(db.parent / "api-token")
        )),
    }


def validate_receipt_file(
    receipt_path: Path,
    *,
    repo_root: Path = REPO_ROOT,
    installation_file: Path | None = None,
    db_path: Path | None = None,
    binary_path: Path | None = None,
    release_manifest_path: Path | None = None,
    token_file: Path | None = None,
    service_url: str | None = None,
    now: datetime | None = None,
    allow_session_discovery_drift: bool = False,
) -> dict[str, Any]:
    try:
        receipt = json.loads(Path(receipt_path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ConformanceError("conformance receipt is unavailable or invalid") from exc
    defaults = _defaults(repo_root)
    evidence = collect_evidence(
        repo_root=repo_root,
        installation_file=installation_file or defaults["installation_file"],
        db_path=db_path or defaults["db_path"],
        binary_path=binary_path or defaults["binary_path"],
        release_manifest_path=release_manifest_path or defaults["release_manifest_path"],
        token_file=token_file or defaults["token_file"],
        service_url=service_url,
        focused_tests=receipt.get("focused_tests", {}),
    )
    return validate_receipt_payload(
        receipt,
        current_evidence=evidence,
        now=now or datetime.now(timezone.utc),
        allow_session_discovery_drift=allow_session_discovery_drift,
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    parser.add_argument("--installation-file", type=Path)
    parser.add_argument("--db", type=Path)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--release-manifest", type=Path)
    parser.add_argument("--token-file", type=Path)
    parser.add_argument("--service-url")
    subparsers = parser.add_subparsers(dest="command", required=True)
    issue = subparsers.add_parser("issue")
    issue.add_argument("--out", type=Path, required=True)
    issue.add_argument("--ttl-seconds", type=int, default=DEFAULT_TTL_SECONDS)
    validate = subparsers.add_parser("validate")
    validate.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args(argv)
    defaults = _defaults(args.repo_root)
    common = {
        "repo_root": args.repo_root,
        "installation_file": args.installation_file or defaults["installation_file"],
        "db_path": args.db or defaults["db_path"],
        "binary_path": args.binary or defaults["binary_path"],
        "release_manifest_path": args.release_manifest or defaults["release_manifest_path"],
        "token_file": args.token_file or defaults["token_file"],
        "service_url": args.service_url,
    }
    try:
        if args.command == "issue":
            evidence = collect_evidence(**common)
            receipt = issue_receipt(
                evidence,
                now=datetime.now(timezone.utc),
                ttl_seconds=args.ttl_seconds,
            )
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_text(
                json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
        else:
            validate_receipt_file(args.receipt, **common)
            receipt = json.loads(args.receipt.read_text(encoding="utf-8"))
    except (ConformanceError, cross_machine.CrossMachineAdaptError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, sort_keys=True))
        return 1
    print(json.dumps({
        "ok": True,
        "installation_id": receipt["installation_id"],
        "receipt_sha256": receipt["receipt_sha256"],
        "expires_at": receipt["expires_at"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
