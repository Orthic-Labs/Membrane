"""Mac-only launchd registrar. launchd is Crypt's only lifecycle owner."""
from __future__ import annotations

import subprocess
from pathlib import Path

from crypt_service_launchd import DEFAULT_CRYPT_SERVE_LABEL, render_crypt_launchd_plist


def _uid(runner) -> str:
    result = runner(["id", "-u"], capture_output=True, text=True)
    if result.returncode != 0 or not result.stdout.strip():
        raise RuntimeError("unable to resolve launchd user id")
    return result.stdout.strip()


def setup_crypt_serve_autostart(
    repo: Path, home: Path, port: int, *, mac: bool, win: bool = False,
    label: str = DEFAULT_CRYPT_SERVE_LABEL, runner=None,
) -> None:
    """Replace this launchd unit atomically; foreign port owners fail closed."""
    if win:
        raise ValueError("Crypt launchd registration is macOS-only")
    if not mac:
        raise ValueError("Crypt launchd registration requires macOS")
    runner = runner or subprocess.run
    uid = _uid(runner)
    domain = f"gui/{uid}"
    plist = home / "Library/LaunchAgents" / f"{label}.plist"
    plist.parent.mkdir(parents=True, exist_ok=True)
    (home / "Library/Logs").mkdir(parents=True, exist_ok=True)
    body = render_crypt_launchd_plist(repo, home, port, label=label)
    runner(["launchctl", "bootout", f"{domain}/{label}"], capture_output=True, text=True)
    from crypt_service import wait_for_tcp_port_closed, wait_for_tcp_port_open
    wait_for_tcp_port_closed("127.0.0.1", port)
    plist.write_text(body, encoding="utf-8")
    result = runner(["launchctl", "bootstrap", domain, str(plist)], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{label}: launchctl bootstrap failed — {result.stderr.strip()[:200]}")
    wait_for_tcp_port_open("127.0.0.1", port)
