"""Small direct-runtime boundary for reviewed Taste v2 applies.

It intentionally imports neither legacy Taste nor session mining modules.
"""
from __future__ import annotations
import json
import os
import shutil
import subprocess
from pathlib import Path

STATE_DIR = Path.home() / ".claude" / "morph"

def state_dir() -> Path: return Path(os.environ.get("MORPH_STATE_DIR", STATE_DIR))
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
def run_crypt(args: list[str]) -> bool:
    binary = shutil.which("crypt")
    if not binary: return False
    command = list(args)
    if command and command[0] == "put": command.extend(["--artifact-family", "morph", "--producer", "morph", "--record-type", "preference"])
    try: return subprocess.run([binary, *command], capture_output=True, text=True, timeout=150).returncode == 0
    except (OSError, subprocess.TimeoutExpired): return False
def scanner_available() -> bool: return bool(shutil.which("gitleaks") or shutil.which("detect-secrets"))
