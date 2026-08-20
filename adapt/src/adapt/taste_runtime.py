"""Small direct-runtime boundary for reviewed Taste v2 applies.

It intentionally imports neither legacy Taste nor session mining modules.
"""
from __future__ import annotations
import json
import os
import shutil
import subprocess
from pathlib import Path

from adapt import cross_machine
from adapt import rollback
from adapt.workspace_runtime import workspace_root

STATE_DIR = Path.home() / ".claude" / "adapt"

def state_dir() -> Path: return Path(os.environ.get("ADAPT_STATE_DIR", STATE_DIR))
def state_path() -> Path: return state_dir() / "taste-v2-state.json"
def rules_path() -> Path: return state_dir() / "rules.json"
def load_json(path: Path, default: dict | None = None) -> dict:
    try: return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError): return default or {}
def write_json_atomic(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temporary, path)
def run_cortex(args: list[str]) -> bool:
    binary = shutil.which("cortex")
    if not binary: return False
    command = list(args)
    if command and command[0] == "put": command.extend(["--artifact-family", "adapt", "--producer", "adapt", "--record-type", "preference"])
    try: return subprocess.run([binary, *command], capture_output=True, text=True, timeout=150).returncode == 0
    except (OSError, subprocess.TimeoutExpired): return False
def scanner_available() -> bool: return bool(shutil.which("gitleaks") or shutil.which("detect-secrets"))


def installation_file() -> Path:
    """Return shared multiwriter installation identity location."""
    override = os.environ.get("ADAPT_INSTALLATION_FILE", "").strip()
    if override:
        return Path(override)
    return workspace_root() / "tools/.cache/memory/installation.json"


def multiwriter_context(*, manifest_body: dict, required: bool = False) -> tuple[str, dict] | None:
    """Delegate identity & canonical-pool loading to established helpers."""
    identity_path = installation_file()
    if not identity_path.is_file():
        if required:
            raise cross_machine.CrossMachineAdaptError(
                "multiwriter manifest requires a local schema-v2 installation identity"
            )
        return None
    installation_id = cross_machine.load_installation_id(identity_path)
    db_path = rollback._discover_db_path(manifest_body)
    if db_path is None:
        raise cross_machine.CrossMachineAdaptError("canonical Cortex DB is unavailable")
    return installation_id, cross_machine.load_canonical_rules(db_path)


def qualify_session_sources(session_refs: list[dict], installation_id: str) -> list[str]:
    """Delegate source identity qualification to cross-machine contract."""
    return [
        ref["source_id"]
        if str(ref.get("source_id", "")).startswith(f"install:{installation_id}:")
        else cross_machine.qualify_source_session(
            installation_id,
            ref["tool"],
            ref.get("source_key") or ref.get("session_id") or ref["source_id"],
        )
        for ref in session_refs
    ]


def validate_multiwriter_binding(
    manifest_body: dict, *, installation_id: str, canonical_rules: dict
) -> None:
    cross_machine.validate_multiwriter_binding(
        manifest_body,
        installation_id=installation_id,
        canonical_rules=canonical_rules,
    )


CrossMachineAdaptError = cross_machine.CrossMachineAdaptError
