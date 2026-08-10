"""Membrane-owned macOS launchd plist renderers for the Crypt workspace service trio
(resident serve, daily sync, replication). Pure stdlib only. Re-exported by
``crypt_service.py``. The registrars that use these renderers live in the sibling
module ``crypt_service_registrars`` — split out purely to keep each module under its
reviewable-size ceiling, not to move behavior out of Membrane.
"""
from __future__ import annotations

import shutil
from pathlib import Path
from xml.sax.saxutils import escape

DEFAULT_CRYPT_SERVE_LABEL = "com.membrane.workspace.crypt-serve"
DEFAULT_CRYPT_DAILY_LABEL = "com.membrane.workspace.crypt-daily"
DEFAULT_CRYPT_REPLICATION_LABEL = "com.membrane.workspace.crypt-replication"


def render_crypt_launchd_plist(repo: Path, home: Path, port: int, *, label: str = DEFAULT_CRYPT_SERVE_LABEL) -> str:
    """Render the resident Mac service contract without invoking the Codex hook recursively."""
    binary = escape((repo / "tools/bin/crypt-service").resolve(strict=False).as_posix())
    database = escape((repo / "tools/.cache/memory/crypt-engine.db").as_posix())
    token = escape((repo / "tools/.cache/memory/api-token").as_posix())
    runtime = escape((repo / "tools/bin/libonnxruntime.dylib").as_posix())
    model_cache = escape((repo / "tools/.cache/fastembed").as_posix())
    python = escape((repo / ".venv-tools/bin/python").as_posix())
    node = escape(shutil.which("node") or "/usr/bin/node")
    log_path = escape((home / "Library/Logs/crypt-serve.log").as_posix())
    working_directory = escape(home.as_posix())
    workspace_root = escape(repo.as_posix())
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
  </array>
  <key>WorkingDirectory</key><string>{working_directory}</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>{log_path}</string>
  <key>StandardErrorPath</key><string>{log_path}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>WORKSPACE_ROOT</key><string>{workspace_root}</string>
    <key>CRYPT_DB</key><string>{database}</string>
    <key>CRYPT_API_TOKEN_FILE</key><string>{token}</string>
    <key>CRYPT_PORT</key><string>{port}</string>
    <key>ORT_DYLIB_PATH</key><string>{runtime}</string>
    <key>HF_HOME</key><string>{model_cache}</string>
    <key>HF_HUB_OFFLINE</key><string>1</string>
    <key>PYTHON</key><string>{python}</string>
    <key>NODE_BIN</key><string>{node}</string>
  </dict>
</dict>
</plist>
"""


def render_crypt_daily_launchd_plist(repo: Path, home: Path, *, label: str = DEFAULT_CRYPT_DAILY_LABEL) -> str:
    """Render the one-shot mirror job without assigning installation identity."""
    script = escape((repo / "tools/pipelines/memory/daily-sync.sh").as_posix())
    path = escape(f"{home.as_posix()}/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin")
    database = escape((repo / "tools/.cache/memory/crypt-engine.db").as_posix())
    token = escape((repo / "tools/.cache/memory/api-token").as_posix())
    runtime = escape((repo / "tools/bin/libonnxruntime.dylib").as_posix())
    model_cache = escape((repo / "tools/.cache/fastembed").as_posix())
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array><string>/bin/sh</string><string>{script}</string></array>
  <key>StartCalendarInterval</key>
  <dict><key>Hour</key><integer>10</integer><key>Minute</key><integer>0</integer></dict>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{path}</string>
    <key>CRYPT_DB</key><string>{database}</string>
    <key>CRYPT_API_TOKEN_FILE</key><string>{token}</string>
    <key>ORT_DYLIB_PATH</key><string>{runtime}</string>
    <key>HF_HOME</key><string>{model_cache}</string>
  </dict>
</dict>
</plist>
"""


def render_crypt_replication_launchd_plist(
    repo: Path, home: Path, interval_seconds: int = 900, *, label: str = DEFAULT_CRYPT_REPLICATION_LABEL,
) -> str:
    """Render the isolated replication-only launchd job (default: every 15 minutes)."""
    if interval_seconds < 60:
        raise ValueError("replication interval must be at least 60 seconds")
    script = escape((repo / "tools/pipelines/memory/replication-only.sh").as_posix())
    path = escape(f"{home.as_posix()}/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin")
    database = escape((repo / "tools/.cache/memory/crypt-engine.db").as_posix())
    token = escape((repo / "tools/.cache/memory/api-token").as_posix())
    runtime = escape((repo / "tools/bin/libonnxruntime.dylib").as_posix())
    model_cache = escape((repo / "tools/.cache/fastembed").as_posix())
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array><string>/bin/sh</string><string>{script}</string></array>
  <key>StartInterval</key><integer>{interval_seconds}</integer>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{path}</string>
    <key>CRYPT_DB</key><string>{database}</string>
    <key>CRYPT_API_TOKEN_FILE</key><string>{token}</string>
    <key>ORT_DYLIB_PATH</key><string>{runtime}</string>
    <key>HF_HOME</key><string>{model_cache}</string>
    <key>HF_HUB_OFFLINE</key><string>1</string>
  </dict>
</dict>
</plist>
"""
