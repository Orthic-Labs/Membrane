"""Membrane-owned launchd contract for singleton Crypt service ownership."""
from __future__ import annotations

import shutil
from pathlib import Path
from xml.sax.saxutils import escape

# Preserve provisioned workspace identity so adoption replaces its owner instead
# of creating a second KeepAlive service against the same loopback port.
DEFAULT_CRYPT_SERVE_LABEL = "com.adrian.crypt-serve"


def render_crypt_launchd_plist(
    repo: Path, home: Path, port: int, *, label: str = DEFAULT_CRYPT_SERVE_LABEL,
) -> str:
    """Render launchd's sole Crypt lifecycle contract; never embeds hook behavior."""
    if not 1 <= int(port) <= 65535:
        raise ValueError("Crypt port must be within 1..=65535")
    binary = escape((repo / "tools/bin/membrane").resolve(strict=False).as_posix())
    database = escape((repo / "tools/.cache/memory/crypt-engine.db").as_posix())
    token = escape((repo / "tools/.cache/memory/api-token").as_posix())
    runtime = escape((repo / "tools/bin/libonnxruntime.dylib").as_posix())
    model_cache = escape((repo / "tools/.cache/fastembed").as_posix())
    python = escape((repo / ".venv-tools/bin/python").as_posix())
    node = escape(shutil.which("node") or "/usr/bin/node")
    log_path = escape((home / "Library/Logs/crypt-serve.log").as_posix())
    workspace_root = escape(repo.resolve(strict=False).as_posix())
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{escape(label)}</string>
  <key>ProgramArguments</key><array><string>{binary}</string><string>supervisor-child</string></array>
  <key>WorkingDirectory</key><string>{escape(home.as_posix())}</string>
  <key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>{log_path}</string><key>StandardErrorPath</key><string>{log_path}</string>
  <key>EnvironmentVariables</key><dict>
    <key>WORKSPACE_ROOT</key><string>{workspace_root}</string><key>CRYPT_DB</key><string>{database}</string>
    <key>CRYPT_API_TOKEN_FILE</key><string>{token}</string><key>CRYPT_PORT</key><string>{int(port)}</string>
    <key>ORT_DYLIB_PATH</key><string>{runtime}</string><key>HF_HOME</key><string>{model_cache}</string>
    <key>HF_HUB_OFFLINE</key><string>1</string><key>PYTHON</key><string>{python}</string><key>NODE_BIN</key><string>{node}</string>
  </dict>
</dict></plist>
'''
