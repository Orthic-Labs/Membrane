#!/usr/bin/env python3
"""Cross-platform orchestration for Crypt vector backend bake-off."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[1]
CONFIG = ROOT / "config" / "round1-v1.json"
SMOKE_MANIFEST = ROOT / "fixtures" / "smoke-v1.manifest.json"
RUNNERS = ("control", "sqlite-stable", "lance", "sqlite-alpha")
EXECUTABLE_SUFFIXES = {".py", ".rs", ".toml", ".lock", ".json", ".sh", ".ps1"}
EXACT_ARMS = {"A", "B", "C1", "C2", "D"}


class HarnessError(RuntimeError):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def executable_inputs(root: Path = ROOT) -> list[Path]:
    paths: list[Path] = []
    for path in root.rglob("*"):
        if not path.is_file() or (
            path.suffix not in EXECUTABLE_SUFFIXES and path.name != ".gitattributes"
        ):
            continue
        if any(part in {"target", ".results", "__pycache__"} for part in path.parts):
            continue
        paths.append(path)
    return sorted(paths, key=lambda item: item.relative_to(root).as_posix())


def input_digest(paths: Iterable[Path], root: Path = ROOT) -> str:
    digest = hashlib.sha256()
    for path in sorted(paths, key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix().encode()
        payload = path.read_bytes()
        digest.update(len(relative).to_bytes(4, "little"))
        digest.update(relative)
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


def command_output(command: list[str], cwd: Path = ROOT) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            check=True,
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise HarnessError(f"preflight command failed: {' '.join(command)}: {error}") from error
    return result.stdout.strip()


def parse_version(text: str) -> tuple[int, ...]:
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", text)
    if not match:
        raise HarnessError(f"unrecognized version: {text}")
    return tuple(int(part) for part in match.groups())


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise HarnessError(f"invalid JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise HarnessError(f"expected JSON object: {path}")
    return value


def preflight() -> dict[str, Any]:
    if sys.version_info < (3, 9):
        raise HarnessError("Python 3.9+ required")
    config = load_json(CONFIG)
    manifest = load_json(SMOKE_MANIFEST)
    cargo = command_output(["cargo", "--version"])
    rustc = command_output(["rustc", "--version"])
    minimum = parse_version(config["dependencies"]["rustMinimum"])
    if parse_version(rustc) < minimum:
        raise HarnessError(f"rustc {minimum} or newer required: {rustc}")
    locks = [ROOT / name / "Cargo.lock" for name in (*RUNNERS, "common", "protoc")]
    missing = [str(path) for path in locks if not path.is_file()]
    if missing:
        raise HarnessError(f"missing committed lockfiles: {', '.join(missing)}")
    if manifest["config"] != "../config/round1-v1.json":
        raise HarnessError("smoke manifest config path drift")
    return {
        "config": config,
        "manifest": manifest,
        "cargo": cargo,
        "rustc": rustc,
        "gitHead": command_output(["git", "rev-parse", "HEAD"], REPO),
    }


def host_slug() -> str:
    raw = f"{platform.system()}-{platform.machine()}-{platform.node()}".lower()
    return re.sub(r"[^a-z0-9._-]+", "-", raw).strip("-") or "unknown-host"


def resolve_protoc() -> str:
    output = command_output(
        [
            "cargo",
            "run",
            "--manifest-path",
            str(ROOT / "protoc" / "Cargo.toml"),
            "--locked",
            "--quiet",
        ]
    )
    path = Path(output.splitlines()[-1]) if output else Path()
    if not path.is_file():
        raise HarnessError(f"vendored protoc resolution failed: {output}")
    return str(path)


def run_command(
    command: list[str], *, env: dict[str, str], timeout_seconds: int, log_path: Path
) -> None:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    with log_path.open("a", encoding="utf-8") as log:
        log.write(f"RUN {' '.join(command)}\n")
        log.flush()
        try:
            result = subprocess.run(
                command,
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=timeout_seconds,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise HarnessError(f"runner timeout after {timeout_seconds}s: {command[0]}") from error
        except OSError as error:
            raise HarnessError(f"runner launch failed: {error}") from error
        elapsed = time.monotonic() - started
        log.write(f"EXIT {result.returncode} elapsedSeconds={elapsed:.3f}\n")
    if result.returncode != 0:
        raise HarnessError(f"runner failed ({result.returncode}); see {log_path}")


def validate_bundle(
    bundle: dict[str, Any], *, runner: str, cell: str, expected_fixture: str | None
) -> str:
    required = {
        "schemaVersion",
        "generatorId",
        "runner",
        "cellId",
        "fixtureSha256",
        "rows",
        "queries",
        "dimension",
        "arms",
    }
    if not required.issubset(bundle):
        raise HarnessError(f"{runner}/{cell} result schema incomplete")
    if bundle["schemaVersion"] != 1 or bundle["generatorId"] != "crypt-vector-fixture-v1":
        raise HarnessError(f"{runner}/{cell} result schema drift")
    if bundle["runner"] != runner or bundle["cellId"] != cell:
        raise HarnessError(f"{runner}/{cell} result identity mismatch")
    fixture = bundle["fixtureSha256"]
    if expected_fixture and fixture != expected_fixture:
        raise HarnessError(f"{runner}/{cell} fixture drift: {fixture} != {expected_fixture}")
    if not isinstance(bundle["arms"], list) or not bundle["arms"]:
        raise HarnessError(f"{runner}/{cell} has no arms")
    for arm in bundle["arms"]:
        if arm.get("arm") in EXACT_ARMS and not arm.get("exact"):
            raise HarnessError(f"exact arm mislabeled: {arm.get('arm')}")
        if arm.get("arm") not in EXACT_ARMS and "meanRecallAtK" not in arm.get("config", {}):
            raise HarnessError(f"ANN arm lacks recall: {arm.get('arm')}")
        if arm.get("arm") == "G" and not arm.get("researchOnly"):
            raise HarnessError("DiskANN arm must remain research-only")
    return fixture


def resume_valid(entry: dict[str, Any] | None, output: Path, digest: str) -> bool:
    if not entry or entry.get("inputDigest") != digest or not output.is_file():
        return False
    return entry.get("resultSha256") == sha256_file(output)


def runner_command(runner: str, cell: str, output: Path) -> list[str]:
    return [
        "cargo",
        "run",
        "--manifest-path",
        str(ROOT / runner / "Cargo.toml"),
        "--locked",
        "--release",
        "--",
        "--config",
        str(CONFIG),
        "--cell",
        cell,
        "--output",
        str(output),
    ]


def run_mode(mode: str, output_dir: Path, force: bool, timeout_seconds: int) -> Path:
    facts = preflight()
    config = facts["config"]
    cells = ["smoke"] if mode == "smoke" else [cell["id"] for cell in config["cells"] if cell["id"].startswith("round1-")]
    if not cells:
        raise HarnessError(f"no cells for mode {mode}")
    inputs = executable_inputs()
    digest = input_digest(inputs)
    expected_smoke = facts["manifest"]["fixtureSha256"]
    receipt_path = output_dir / "receipt.json"
    prior = load_json(receipt_path) if receipt_path.is_file() else {}
    prior_entries = {
        (entry.get("cell"), entry.get("runner")): entry for entry in prior.get("results", [])
    }
    results: list[dict[str, Any]] = []
    protoc = resolve_protoc()
    env = dict(os.environ)
    env["PROTOC"] = protoc
    env["PYTHON"] = sys.executable
    output_dir.mkdir(parents=True, exist_ok=True)
    log_path = output_dir / "harness.log"

    for cell in cells:
        fixture_for_cell: str | None = expected_smoke if cell == "smoke" else None
        observed_fixture: str | None = None
        for runner in RUNNERS:
            output = output_dir / cell / f"{runner}.json"
            key = (cell, runner)
            if not force and resume_valid(prior_entries.get(key), output, digest):
                bundle = load_json(output)
            else:
                print(f"RUN cell={cell} runner={runner}", flush=True)
                run_command(
                    runner_command(runner, cell, output),
                    env=env,
                    timeout_seconds=timeout_seconds,
                    log_path=log_path,
                )
                bundle = load_json(output)
            fixture = validate_bundle(
                bundle,
                runner=runner,
                cell=cell,
                expected_fixture=fixture_for_cell or observed_fixture,
            )
            observed_fixture = observed_fixture or fixture
            results.append(
                {
                    "cell": cell,
                    "runner": runner,
                    "path": output.relative_to(output_dir).as_posix(),
                    "fixtureSha256": fixture,
                    "resultSha256": sha256_file(output),
                    "inputDigest": digest,
                }
            )
            atomic_json(
                receipt_path,
                make_receipt(mode, facts, inputs, digest, results),
            )
            print(f"PASS cell={cell} runner={runner} fixture={fixture}", flush=True)
    atomic_json(receipt_path, make_receipt(mode, facts, inputs, digest, results))
    print(f"PASS mode={mode} receipt={receipt_path}", flush=True)
    return receipt_path


def make_receipt(
    mode: str,
    facts: dict[str, Any],
    inputs: list[Path],
    digest: str,
    results: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "schemaVersion": 1,
        "mode": mode,
        "status": "complete" if len(results) == (4 if mode == "smoke" else 24) else "partial",
        "inputDigest": digest,
        "configSha256": sha256_file(CONFIG),
        "manifestSha256": sha256_file(SMOKE_MANIFEST),
        "inputs": [path.relative_to(ROOT).as_posix() for path in inputs],
        "host": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "node": platform.node(),
            "python": platform.python_version(),
            "rustc": facts["rustc"],
            "cargo": facts["cargo"],
        },
        "gitHead": facts["gitHead"],
        "results": results,
    }


def candidate_ids(bundle: dict[str, Any], arm_name: str) -> list[list[int]]:
    arm = next((item for item in bundle["arms"] if item["arm"] == arm_name), None)
    if arm is None:
        raise HarnessError(f"missing exact arm {arm_name}")
    return [measurement["candidateIds"] for measurement in arm["measurements"]]


def compare_receipts(left_path: Path, right_path: Path) -> dict[str, Any]:
    left = load_json(left_path)
    right = load_json(right_path)
    for key in ("schemaVersion", "mode", "inputDigest", "configSha256", "manifestSha256"):
        if left.get(key) != right.get(key):
            raise HarnessError(f"cross-host drift in {key}")
    if left.get("status") != "complete" or right.get("status") != "complete":
        raise HarnessError("both host receipts must be complete")
    left_entries = {(item["cell"], item["runner"]): item for item in left["results"]}
    right_entries = {(item["cell"], item["runner"]): item for item in right["results"]}
    if left_entries.keys() != right_entries.keys():
        raise HarnessError("cross-host result matrix drift")
    exact_checks = 0
    for key in sorted(left_entries):
        a = left_entries[key]
        b = right_entries[key]
        if a["fixtureSha256"] != b["fixtureSha256"]:
            raise HarnessError(f"cross-host fixture drift: {key}")
        left_bundle = load_json(left_path.parent / a["path"])
        right_bundle = load_json(right_path.parent / b["path"])
        left_arms = {arm["arm"] for arm in left_bundle["arms"]}
        right_arms = {arm["arm"] for arm in right_bundle["arms"]}
        if left_arms != right_arms:
            raise HarnessError(f"cross-host arm drift: {key}")
        for arm in sorted(left_arms & EXACT_ARMS):
            if candidate_ids(left_bundle, arm) != candidate_ids(right_bundle, arm):
                raise HarnessError(f"cross-host exact candidate drift: {key}/{arm}")
            exact_checks += 1
    return {
        "schemaVersion": 1,
        "status": "pass",
        "inputDigest": left["inputDigest"],
        "leftHost": left["host"],
        "rightHost": right["host"],
        "cells": len({key[0] for key in left_entries}),
        "runnerResults": len(left_entries),
        "exactArmChecks": exact_checks,
    }


def default_output(mode: str) -> Path:
    return ROOT / ".results" / f"{mode}-{host_slug()}"


def parser() -> argparse.ArgumentParser:
    command = argparse.ArgumentParser(description=__doc__)
    subcommands = command.add_subparsers(dest="command", required=True)
    for name in ("smoke", "round1"):
        sub = subcommands.add_parser(name)
        sub.add_argument("--output", type=Path)
        sub.add_argument("--force", action="store_true")
        sub.add_argument("--timeout-seconds", type=int, default=14_400)
    compare = subcommands.add_parser("compare")
    compare.add_argument("left", type=Path)
    compare.add_argument("right", type=Path)
    compare.add_argument("--output", type=Path)
    return command


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "compare":
            report = compare_receipts(args.left.resolve(), args.right.resolve())
            output = args.output or ROOT / ".results" / "cross-host-comparison.json"
            atomic_json(output, report)
            print(f"PASS compare={output}")
        else:
            output = (args.output or default_output(args.command)).resolve()
            run_mode(args.command, output, args.force, args.timeout_seconds)
    except HarnessError as error:
        print(f"FAIL {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
