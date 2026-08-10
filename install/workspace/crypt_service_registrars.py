"""Membrane-owned macOS/Windows autostart registrars for the Crypt workspace service
trio (resident serve, daily sync, replication). Pure stdlib only. Re-exported by
``crypt_service.py``; renderers live in ``crypt_service_launchd``.

Each registrar accepts an optional ``migrate_from_label`` (never defaulted here — the
caller supplies whatever pre-Membrane identifier its own installer used) so a machine
still running that old launchd unit converges onto the new ``com.membrane.workspace.*``
one: the old plist is unloaded and its file deleted (not just unloaded — a stale plist
on disk is exactly the double-ownership bug class this migration exists to eliminate),
then the new plist takes its place.
"""
from __future__ import annotations

import shutil
import subprocess
import time
from pathlib import Path

from crypt_service_launchd import (
    DEFAULT_CRYPT_DAILY_LABEL,
    DEFAULT_CRYPT_REPLICATION_LABEL,
    DEFAULT_CRYPT_SERVE_LABEL,
    render_crypt_daily_launchd_plist,
    render_crypt_launchd_plist,
    render_crypt_replication_launchd_plist,
)


def bootstrap_launch_agent(domain: str, plist: Path, *, attempts: int = 4, runner=None) -> subprocess.CompletedProcess:
    """Retry launchd's transient unload-to-bootstrap I/O race."""
    runner = runner or subprocess.run
    result = None
    for attempt in range(attempts):
        result = runner(["launchctl", "bootstrap", domain, str(plist)], capture_output=True, text=True)
        if result.returncode == 0 or "Input/output error" not in result.stderr:
            return result
        if attempt + 1 < attempts:
            time.sleep(0.25 * (2 ** attempt))
    assert result is not None
    return result


def _uid(runner) -> str:
    return runner(["id", "-u"], capture_output=True, text=True).stdout.strip()


def _retire_label(label: str, home: Path, uid: str, runner) -> bool:
    """Bootout + delete a launchd unit's plist. Returns True if launchd owned it."""
    result = runner(["launchctl", "bootout", f"gui/{uid}/{label}"], capture_output=True)
    old_plist = home / "Library/LaunchAgents" / f"{label}.plist"
    if old_plist.exists():
        old_plist.unlink()
    return result.returncode == 0


def migrate_macos_label(
    old_label: str, new_label: str, new_plist_path: Path, new_plist_body: str, *,
    home: Path, uid: str | None = None, runner=None,
) -> subprocess.CompletedProcess:
    """Retire ``old_label``'s launchd registration and install ``new_label`` in its place.

    Idempotent: once old_label is gone, the bootout+unlink are no-ops and only the
    new_label bootstrap remains, itself idempotent.
    """
    runner = runner or subprocess.run
    uid = uid if uid is not None else _uid(runner)
    _retire_label(old_label, home, uid, runner)
    new_plist_path.parent.mkdir(parents=True, exist_ok=True)
    new_plist_path.write_text(new_plist_body, encoding="utf-8")
    result = bootstrap_launch_agent(f"gui/{uid}", new_plist_path, runner=runner)
    if result.returncode != 0:
        raise RuntimeError(f"{new_label}: launchctl bootstrap failed — {result.stderr.strip()[:200]}")
    return result


def _win_ps1_dispatch(repo: Path, installer_ps1: Path | None, extra_args: list[str], runner) -> subprocess.CompletedProcess | None:
    installer = installer_ps1 or (repo / "membrane/install/workspace/install-windows-tasks.ps1")
    shell = shutil.which("pwsh") or shutil.which("powershell")
    if not shell:
        return None
    command = [
        shell, "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", str(installer), "-WorkspaceRoot", str(repo), *extra_args,
    ]
    return runner(command, capture_output=True, text=True)


def setup_crypt_serve_autostart(
    repo: Path, home: Path, port: int, *, win: bool, mac: bool, enable_daily: bool = True,
    migrate_from_label: str | None = None, label: str = DEFAULT_CRYPT_SERVE_LABEL,
    installer_ps1: Path | None = None, runner=None,
) -> None:
    """Keep the local Crypt browse/recall service warm after login. Idempotent."""
    runner = runner or subprocess.run
    if win:
        result = _win_ps1_dispatch(repo, installer_ps1, ["-EnableDaily"] if enable_daily else [], runner)
        if result is None:
            print("[membrane-workspace] crypt-serve: PowerShell not found — skipped")
            return
        print("[membrane-workspace] crypt-serve + daily-sync: Task Scheduler tasks registered"
              if result.returncode == 0
              else f"[membrane-workspace] crypt tasks: registration failed — {result.stderr.strip()[:200]}")
        return
    if not mac:
        print("[membrane-workspace] crypt-serve: linux — add a user service manually if wanted")
        return

    plist = home / "Library/LaunchAgents" / f"{label}.plist"
    plist.parent.mkdir(parents=True, exist_ok=True)
    (repo / "tools/.cache/metrics").mkdir(parents=True, exist_ok=True)
    (home / "Library/Logs").mkdir(parents=True, exist_ok=True)
    body = render_crypt_launchd_plist(repo, home, port, label=label)

    uid = _uid(runner)
    # Foreign-supervisor case: when Membrane Hub (not launchd) owns crypt-service, bootout
    # is a no-op, the port never closes, and launchd cannot bind it either. That is a
    # healthy machine, not a failure — only raise once launchd itself owned either label.
    owns_new = runner(["launchctl", "bootout", f"gui/{uid}/{label}"], capture_output=True).returncode == 0
    owns_old = _retire_label(migrate_from_label, home, uid, runner) if migrate_from_label else False
    plist.write_text(body, encoding="utf-8")

    if not (owns_new or owns_old):
        print(f"[membrane-workspace] crypt-serve: another supervisor owns :{port} — plist written, "
              "not bootstrapped (leaving the running service alone)")
        return
    from crypt_service import wait_for_tcp_port_closed, wait_for_tcp_port_open

    wait_for_tcp_port_closed("127.0.0.1", port)
    r = runner(["launchctl", "bootstrap", f"gui/{uid}", str(plist)], capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError("crypt-serve: launchctl failed — " + r.stderr.strip()[:200])
    wait_for_tcp_port_open("127.0.0.1", port)
    print("[membrane-workspace] crypt-serve: launchd agent registered (RunAtLoad)")


def _register_scheduled_plist(
    home: Path, body: str, *, enable: bool, label: str,
    migrate_from_label: str | None, runner, name: str,
) -> None:
    plist = home / "Library/LaunchAgents" / f"{label}.plist"
    uid = _uid(runner)
    if migrate_from_label:
        _retire_label(migrate_from_label, home, uid, runner)
    else:
        runner(["launchctl", "bootout", f"gui/{uid}/{label}"], capture_output=True)
    plist.parent.mkdir(parents=True, exist_ok=True)
    plist.write_text(body, encoding="utf-8")
    state = "enable" if enable else "disable"
    result = runner(["launchctl", state, f"gui/{uid}/{label}"], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"launchctl {state} failed for {label}: {result.stderr.strip()[:200]}")
    if enable:
        r = runner(["launchctl", "bootstrap", f"gui/{uid}", str(plist)], capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(f"{name}: launchd bootstrap failed — {r.stderr.strip()[:200]}")
    print(f"[membrane-workspace] {name}: launchd {'registered' if enable else 'kept unloaded'}")


def setup_daily_sync(
    repo: Path, home: Path, *, win: bool, mac: bool, enable: bool = True,
    migrate_from_label: str | None = None, label: str = DEFAULT_CRYPT_DAILY_LABEL,
    runner=None,
) -> None:
    """Register tools/pipelines/memory/daily-sync.sh to run daily at 10:00. Idempotent."""
    runner = runner or subprocess.run
    if win:
        return  # the Windows installer registers this atomically in setup_crypt_serve_autostart()
    if not mac:
        print("[membrane-workspace] daily-sync: linux — add a cron entry manually if wanted")
        return
    body = render_crypt_daily_launchd_plist(repo, home, label=label)
    _register_scheduled_plist(
        home, body, enable=enable, label=label,
        migrate_from_label=migrate_from_label, runner=runner, name="daily-sync",
    )


def setup_replication_schedule(
    repo: Path, home: Path, *, win: bool, mac: bool, enable: bool = True, interval_seconds: int = 900,
    migrate_from_label: str | None = None, label: str = DEFAULT_CRYPT_REPLICATION_LABEL,
    installer_ps1: Path | None = None, runner=None,
) -> None:
    """Register the separately named replication-only schedule; never touches crypt-daily."""
    runner = runner or subprocess.run
    if win:
        minutes = max(1, int(round(interval_seconds / 60)))
        result = _win_ps1_dispatch(
            repo, installer_ps1,
            ["-EnableReplication", "-ReplicationIntervalMinutes", str(minutes)], runner,
        )
        if result is None:
            raise RuntimeError("PowerShell not found — cannot register replication schedule")
        if result.returncode != 0:
            raise RuntimeError("replication schedule registration failed — " + result.stderr.strip()[:200])
        print(f"[membrane-workspace] crypt-replication: Task Scheduler registered (every {minutes} minutes)")
        return
    if not mac:
        print("[membrane-workspace] crypt-replication: linux — add a cron entry manually if wanted")
        return
    body = render_crypt_replication_launchd_plist(repo, home, interval_seconds, label=label)
    _register_scheduled_plist(
        home, body, enable=enable, label=label,
        migrate_from_label=migrate_from_label, runner=runner, name="crypt-replication",
    )
