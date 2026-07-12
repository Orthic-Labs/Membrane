"""Session transcript discovery + user-turn extraction for adapt.

Sources: Claude Code (~/.claude/projects/*/*.jsonl) and Codex
(~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl). Only user-authored turns are
kept; injected wrappers (<...>), tool results, meta, caveat and sidechain rows
are dropped. Every kept turn passes redact() and scanner_clean() before it
can leave the machine.
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

STATE_DIR = Path.home() / ".claude" / "adapt"
STATE_FILE = STATE_DIR / "state.json"

MIN_TURN_CHARS = 10
MAX_TURN_CHARS = 4000
MAX_TURNS_PER_SESSION = 400

# Scopes never mined: health/medical transcripts are out of bounds (and are not coding preferences).
DENIED_SCOPE_ROOTS = (
    "D--Claude-Health",
    "D--Claude-Health-medical-research-system",
)
EXCLUDED_SCOPE_PATTERNS = (
    re.compile(r"health", re.I),
    re.compile(r"medical", re.I),
    re.compile(r"clinic|patient|dose|injection|drug|diagnos", re.I),
)

SECRET_PATTERNS = [
    re.compile(r"sk-[A-Za-z0-9_-]{16,}"),
    re.compile(r"ghp_[A-Za-z0-9]{20,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"xox[bap]-[A-Za-z0-9-]{10,}"),
    re.compile(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]{16,}"),
    re.compile(r"(?i)(password|passphrase|api[_-]?key|secret|token)\s*[:=]\s*\S{6,}"),
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"),
]


def redact(text: str) -> str:
    for pat in SECRET_PATTERNS:
        text = pat.sub("[REDACTED]", text)
    return text


def scanner_clean(text: str) -> bool:
    """Fail-closed hook for detect-secrets/gitleaks over one temporary text file.

    Regex redaction is secondary. Apply/backfill refuses to run unless scanner_available()
    is true. If a scanner reports a finding, the containing turn is dropped before any
    LLM call. Audit only scanner metadata, never the raw secret-bearing turn.
    """
    if not scanner_available():
        return False
    with tempfile.TemporaryDirectory(prefix="adapt-scan-") as d:
        p = Path(d) / "turn.txt"
        p.write_text(text, encoding="utf-8")
        if shutil.which("gitleaks"):
            res = subprocess.run(
                ["gitleaks", "detect", "--no-git", "--redact", "--source", str(d)],
                capture_output=True, text=True, timeout=30)
            if res.returncode != 0:
                return False
        if shutil.which("detect-secrets"):
            res = subprocess.run(
                ["detect-secrets", "scan", str(p)],
                capture_output=True, text=True, timeout=30)
            if res.returncode != 0:
                return False
            try:
                data = json.loads(res.stdout or "{}")
            except json.JSONDecodeError:
                return False
            if (data.get("results") or {}).get(str(p)):
                return False
    return True


def scanner_available() -> bool:
    """True when detect-secrets or gitleaks is callable. Apply/backfill requires this."""
    return bool(shutil.which("detect-secrets") or shutil.which("gitleaks"))


def scope_for_cwd(cwd: str) -> str:
    return cwd.replace(":", "-").replace("\\", "-").replace("/", "-").strip("-") or "global"


def scope_excluded(cwd: str) -> bool:
    scope = scope_for_cwd(cwd)
    return any(scope.startswith(root) for root in DENIED_SCOPE_ROOTS) or any(
        p.search(cwd) for p in EXCLUDED_SCOPE_PATTERNS)


def text_excluded(text: str) -> bool:
    return any(p.search(text) for p in EXCLUDED_SCOPE_PATTERNS)


@dataclass
class Turn:
    text: str
    scope: str


@dataclass
class ParseStats:
    kept_turns: int = 0
    dropped_turns: int = 0
    truncated_turns: int = 0
    scanner_drops: int = 0
    unknown_rows: int = 0


@dataclass
class Session:
    tool: str          # "claude-code" | "codex"
    session_id: str
    path: Path
    cwd: str
    mtime: float
    turns: list[Turn] = field(default_factory=list)
    stats: ParseStats = field(default_factory=ParseStats)


def _keep_turn(text: str) -> bool:
    if len(text) < MIN_TURN_CHARS:
        return False
    if text.startswith("<"):        # <command-name>, <system-reminder>, <recommended_plugins>...
        return False
    if text.startswith("Caveat:"):
        return False
    if text_excluded(text):
        return False
    return True


def _clean(text: str, stats: ParseStats) -> str | None:
    text = text.strip()
    if len(text) > MAX_TURN_CHARS:
        stats.truncated_turns += 1
        text = text[:MAX_TURN_CHARS]
    text = redact(text)
    if not scanner_clean(text):
        stats.scanner_drops += 1
        return None
    return text


def parse_claude_session(path: Path) -> Session | None:
    turns: list[Turn] = []
    stats = ParseStats()
    cwd, sid = "", path.stem
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get("type") != "user" or obj.get("isSidechain") or obj.get("isMeta"):
                if obj.get("type") not in ("user", "assistant"):
                    stats.unknown_rows += 1
                continue
            if obj.get("userType") not in (None, "external"):
                continue
            content = (obj.get("message") or {}).get("content")
            if not isinstance(content, str):
                continue  # list content = tool_result blocks, not typed prompts
            turn_cwd = obj.get("cwd") or cwd
            cwd = cwd or turn_cwd
            sid = obj.get("sessionId") or sid
            text = content.strip()
            if _keep_turn(text):
                cleaned = _clean(text, stats)
                if cleaned is not None:
                    turns.append(Turn(cleaned, scope_for_cwd(turn_cwd)))
                    stats.kept_turns += 1
            else:
                stats.dropped_turns += 1
            if len(turns) >= MAX_TURNS_PER_SESSION:
                stats.dropped_turns += 1
                break
    if not turns:
        return None
    return Session("claude-code", sid, path, cwd, path.stat().st_mtime, turns, stats)


def parse_codex_session(path: Path) -> Session | None:
    turns: list[Turn] = []
    stats = ParseStats()
    cwd, sid = "", path.stem
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            payload = obj.get("payload") or {}
            if not isinstance(payload, dict):
                continue
            if obj.get("type") == "session_meta":
                cwd = payload.get("cwd") or cwd
                sid = payload.get("session_id") or sid
                continue
            if (obj.get("type") != "response_item" or payload.get("type") != "message"
                    or payload.get("role") != "user"):
                if obj.get("type") not in ("response_item", "session_meta", "event_msg"):
                    stats.unknown_rows += 1
                continue
            for item in payload.get("content") or []:
                if isinstance(item, dict) and item.get("type") == "input_text":
                    text = (item.get("text") or "").strip()
                    if _keep_turn(text):
                        cleaned = _clean(text, stats)
                        if cleaned is not None:
                            turns.append(Turn(cleaned, scope_for_cwd(cwd)))
                            stats.kept_turns += 1
                    else:
                        stats.dropped_turns += 1
            if len(turns) >= MAX_TURNS_PER_SESSION:
                stats.dropped_turns += 1
                break
    if not turns:
        return None
    return Session("codex", sid, path, cwd, path.stat().st_mtime, turns, stats)


def load_state() -> dict:
    try:
        return json.loads(STATE_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {"learned": {}}


def save_state(state: dict) -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(json.dumps(state, indent=2), encoding="utf-8")


def discover() -> list[tuple[str, Path]]:
    found: list[tuple[str, Path]] = []
    claude_root = Path.home() / ".claude" / "projects"
    if claude_root.exists():
        for proj in sorted(claude_root.iterdir()):
            if proj.is_dir():
                found.extend(("claude-code", f) for f in sorted(proj.glob("*.jsonl")))
    codex_root = Path.home() / ".codex" / "sessions"
    if codex_root.exists():
        found.extend(("codex", f) for f in sorted(codex_root.glob("*/*/*/*.jsonl")))
    return found


def new_sessions(state: dict, limit: int | None = None) -> list[Session]:
    """Parse sessions not yet learned (or modified since). Excluded/empty ones are
    marked learned immediately so they are never re-parsed."""
    learned = state.setdefault("learned", {})
    out: list[Session] = []
    for tool, path in discover():
        key = path.stem
        mtime = path.stat().st_mtime
        if learned.get(tool, {}).get(key, -1.0) >= mtime:
            continue
        sess = parse_claude_session(path) if tool == "claude-code" else parse_codex_session(path)
        if sess is None or scope_excluded(sess.cwd):
            learned.setdefault(tool, {})[key] = mtime
            continue
        out.append(sess)
        if limit is not None and len(out) >= limit:
            break
    out.sort(key=lambda s: s.mtime)
    return out


def mark_learned(state: dict, sessions: list[Session]) -> None:
    for s in sessions:
        state.setdefault("learned", {}).setdefault(s.tool, {})[s.path.stem] = s.mtime
