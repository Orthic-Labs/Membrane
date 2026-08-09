"""Registry-ready transcript discovery and user-turn extraction for Morph.

Only bounded, known harness transcript roots are searched. Ordinary workspace
files are never considered transcripts. Only user-authored text blocks are
kept; wrappers, tool results, model rows, synthetic prompts, and health content
are dropped before the external-lane batch scanner runs.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.parse
from collections.abc import Mapping
from datetime import datetime, timezone
from dataclasses import dataclass, field
from pathlib import Path

from workspace_runtime import workspace_root

STATE_DIR = Path.home() / ".claude" / "morph"
STATE_FILE = STATE_DIR / "state.json"
# Same anchor as morph.WORKSPACE_ROOT / preference_record._WORKSPACE_ROOT:
# .../tools/pipelines/memory/morph/<file> -> workspace root.
_WORKSPACE_ROOT = workspace_root()

MIN_TURN_CHARS = 10
MAX_TURN_CHARS = 4000
MAX_TURNS_PER_SESSION = 400
# Historical extraction-journal fingerprint only; v2 never prefilters/clips.
PREFERENCE_PREFILTER_VERSION = 1
# Scopes never mined: health/medical transcripts are out of bounds (and are not
# coding preferences). Expressed WORKSPACE-RELATIVE so they hold on every
# machine. The literal Windows-form scopes that used to be listed here
# ("D--Claude-Health") could not match a Mac scope ("Volumes-D-claude-Health")
# at all; the EXCLUDED_SCOPE_PATTERNS regex on the raw cwd still caught those
# sessions, so this was never a live leak, but the roots check was silently
# dead on one of the two machines and must not be relied on as path literals.
DENIED_SCOPE_SUFFIXES = (
    "Health",
    "Health-medical-research-system",
)
EXCLUDED_SCOPE_PATTERNS = (
    re.compile(r"health", re.I),
    re.compile(r"medical", re.I),
    re.compile(r"clinic|patient|dose|injection|drug|diagnos", re.I),
)
SENSITIVE_HEALTH_PATTERNS = (
    re.compile(r"\badrian\.ya?ml\b", re.I),
    re.compile(
        r"\b(workout|wearable|exercise|step count|sleep score|calorie|macros?|"
        r"body[- ]?fat|heart[- ]?rate|blood pressure|glucose|insulin)\b", re.I
    ),
    re.compile(
        r"\b(medication|medicine|supplement|symptom|lab results?|biomarker|"
        r"hormone|peptide|vitamin|mineral|nutrition|dietary|dosage)\b", re.I
    ),
)

SECRET_PATTERNS = [
    re.compile(r"sk-[A-Za-z0-9_-]{16,}"),
    re.compile(r"ghp_[A-Za-z0-9]{20,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"xox[bap]-[A-Za-z0-9-]{10,}"),
    re.compile(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]{16,}"),
    re.compile(r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b"),
    re.compile(r"(?i)(password|passphrase|api[_-]?key|secret|token)\s*[:=]\s*\S{6,}"),
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"),
]


def redact(text: str) -> str:
    for pat in SECRET_PATTERNS:
        text = pat.sub("[REDACTED]", text)
    return text


_SCANNER_CACHE: dict[str, bool] = {}


def scanner_clean(text: str) -> bool:
    """Fail-closed hook for detect-secrets/gitleaks over one temporary text file.

    The original design scanned every kept turn inline; on a corpus with hundreds of
    unlearned sessions and dozens of turns each, the per-turn gitleaks spawn dominated
    runtime (subprocess startup alone was >40s per 10 sessions). Per-iteration cache
    doesn't help: turn text is unique within a session most of the time.

    The privacy guarantee is preserved at the next layer: `scan_batch_for_secrets()`
    scans the full payload once before any external LLM call.
    """
    if not scanner_available():
        return False
    if text in _SCANNER_CACHE:
        return _SCANNER_CACHE[text]
    with tempfile.TemporaryDirectory(prefix="morph-scan-") as d:
        p = Path(d) / "turn.txt"
        p.write_text(text, encoding="utf-8")
        if shutil.which("gitleaks"):
            res = subprocess.run(
                ["gitleaks", "detect", "--no-git", "--redact", "--source", str(d)],
                capture_output=True, text=True, timeout=30)
            if res.returncode != 0:
                _SCANNER_CACHE[text] = False
                return False
        if shutil.which("detect-secrets"):
            res = subprocess.run(
                ["detect-secrets", "scan", str(p)],
                capture_output=True, text=True, timeout=30)
            if res.returncode != 0:
                _SCANNER_CACHE[text] = False
                return False
            try:
                data = json.loads(res.stdout or "{}")
            except json.JSONDecodeError:
                _SCANNER_CACHE[text] = False
                return False
            if (data.get("results") or {}).get(str(p)):
                _SCANNER_CACHE[text] = False
                return False
    _SCANNER_CACHE[text] = True
    return True


def scan_batch_for_secrets(batch: list[tuple[str, str, str]]) -> bool:
    """Scan the entire concat'd batch text ONCE before any external-lane send.

    Returns True on clean, False on scanner-positive. Used as a single pre-send
    guard replacing per-turn scanning — same guarantee at one subprocess call
    per LLM call, not one per turn.
    """
    return scan_text("\n".join(t[2] for t in batch))


def scan_batch_for_secrets_str(text: str) -> bool:
    """Same as `scan_batch_for_secrets` but accepts a single concatenated string
    payload (used by the synthesizer where the payload is a JSON string, not a
    list of (tool, scope, text) tuples).
    """
    return scan_text(text)


def scan_text(text: str) -> bool:
    """Internal: scan one text blob through gitleaks/detect-secrets, fail closed
    if no scanner is installed.
    """
    if not scanner_available():
        return False
    with tempfile.TemporaryDirectory(prefix="morph-scan-") as d:
        p = Path(d) / "payload.txt"
        p.write_text(text, encoding="utf-8")
        if shutil.which("gitleaks"):
            res = subprocess.run(
                ["gitleaks", "detect", "--no-git", "--redact", "--source", str(d)],
                capture_output=True, text=True, timeout=60)
            if res.returncode != 0:
                return False
        if shutil.which("detect-secrets"):
            res = subprocess.run(
                ["detect-secrets", "scan", str(p)],
                capture_output=True, text=True, timeout=60)
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


def dimensions_for_scope(scope: str) -> dict[str, str]:
    """AD1: derive structured scope facets from an observation's scope slug.

    Observations carry the FLATTENED slug (``Turn.scope`` = ``scope_for_cwd``),
    not a real path, so this works off that slug.

    Derives ONLY the repo — what the slug actually proves. Language and
    framework are NOT guessed: a wrong facet NARROWS a rule, and a narrowed rule
    silently stops firing, which is strictly worse than leaving it unqualified
    (unqualified matches everything, the historical behaviour). Those facets
    belong to a caller with real per-file evidence.

    Slug flattening is lossy — a separator and a literal hyphen both become "-",
    so ``...-heardright-ws`` is ambiguous between repo ``heardright`` (subdir
    ``ws``) and repo ``heardright-ws``. Resolved by taking the LONGEST candidate
    that is a real directory under the workspace root, falling back to the first
    segment when nothing matches on disk.
    """
    slug = (scope or "").strip().strip("-")
    if not slug:
        return {}
    workspace_slug = scope_for_cwd(str(_WORKSPACE_ROOT))
    if slug.lower() == workspace_slug.lower():
        return {}  # the workspace root is not a repo
    prefix = workspace_slug.lower() + "-"
    if not slug.lower().startswith(prefix):
        return {}  # a foreign/unknown scope proves nothing about repo identity
    remainder = slug[len(prefix):].strip("-")
    if not remainder:
        return {}
    segments = remainder.split("-")
    for count in range(len(segments), 0, -1):
        candidate = "-".join(segments[:count])
        try:
            if (_WORKSPACE_ROOT / candidate).is_dir():
                return {"repo": candidate}
        except OSError:
            break
    return {"repo": segments[0]}


def local_workspace_scope() -> str:
    """This machine's scope slug for the workspace root.

    Replaces the hardcoded ``"D--Claude"`` literal that used to be the default
    and fallback. That literal is the WINDOWS form; writing it on a Mac minted a
    foreign scope, which the mirror then re-materialised under the local slug and
    produced genuine duplicate rows (three of them, cleaned up 2026-07-26). A
    scope must always name the machine that wrote it, never a hardcoded peer.
    """
    return scope_for_cwd(str(_WORKSPACE_ROOT))


def scope_denied(scope: str) -> bool:
    """Workspace-relative deny check, so it holds on every machine."""
    workspace = local_workspace_scope()
    relative = scope
    if scope.lower().startswith(workspace.lower() + "-"):
        relative = scope[len(workspace) + 1:]
    elif scope.lower() == workspace.lower():
        relative = ""
    return any(
        relative == suffix or relative.startswith(suffix + "-")
        for suffix in DENIED_SCOPE_SUFFIXES
    )


def scope_excluded(cwd: str) -> bool:
    return scope_denied(scope_for_cwd(cwd)) or any(
        p.search(cwd) for p in EXCLUDED_SCOPE_PATTERNS)


def text_excluded(text: str) -> bool:
    return any(
        pattern.search(text)
        for pattern in (*EXCLUDED_SCOPE_PATTERNS, *SENSITIVE_HEALTH_PATTERNS)
    )


@dataclass
class Turn:
    text: str
    scope: str
    observed_at: str | None = None


@dataclass
class Message:
    text: str
    scope: str
    role: str
    observed_at: str | None = None
    author: str | None = None
    recipient: str | None = None


@dataclass
class ParseStats:
    kept_turns: int = 0
    dropped_turns: int = 0
    truncated_turns: int = 0
    scanner_drops: int = 0
    unknown_rows: int = 0


@dataclass
class Session:
    tool: str
    session_id: str
    path: Path
    cwd: str
    mtime: float
    turns: list[Turn] = field(default_factory=list)
    stats: ParseStats = field(default_factory=ParseStats)
    messages: list[Message] = field(default_factory=list)
    agent_role: str | None = None
    thread_source: str = "user"
    parent_thread_id: str | None = None

    def file_sha256(self) -> str:
        """SHA-256 of the session file bytes (stable identifier for reviewers)."""
        import hashlib as _h
        h = _h.sha256()
        with open(self.path, "rb") as fh:
            for chunk in iter(lambda: fh.read(1 << 20), b""):
                h.update(chunk)
        return h.hexdigest()


def _messages_for_turns(turns: list[Turn]) -> list[Message]:
    return [Message(turn.text, turn.scope, "user", turn.observed_at) for turn in turns]


def _keep_turn(text: str) -> bool:
    if len(text) < MIN_TURN_CHARS:
        return False
    if text.startswith("<"):        # <command-name>, <system-reminder>, <recommended_plugins>...
        return False
    if (text.startswith("# AGENTS.md instructions for ")
            and "<INSTRUCTIONS>" in text):
        return False                # Codex-injected workspace authority, not user-authored chat
    if text.startswith("Caveat:"):
        return False
    if text_excluded(text):
        return False
    return True


def _clean(text: str, stats: ParseStats) -> str:
    """Truncate + redact. Scanner check moved to `scan_batch_for_secrets()` to avoid
    one subprocess spawn per kept turn (was 100x+ cost on real corpora).

    Per-turn `scanner_clean()` is preserved as the helper but not called from this
    path; external-lane callers MUST invoke `scan_batch_for_secrets()` before send.
    """
    text = text.strip()
    if len(text) > MAX_TURN_CHARS:
        stats.truncated_turns += 1
        text = text[:MAX_TURN_CHARS]
    return redact(text)


def parse_claude_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
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
                    turns.append(Turn(
                        cleaned,
                        scope_for_cwd(turn_cwd),
                        obj.get("timestamp") if isinstance(obj.get("timestamp"), str) else None,
                    ))
                    stats.kept_turns += 1
            else:
                stats.dropped_turns += 1
            if max_turns is not None and len(turns) >= max_turns:
                stats.dropped_turns += 1
                break
    if not turns:
        return None
    return Session("claude-code", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   messages=_messages_for_turns(turns))


def parse_codex_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
    turns: list[Turn] = []
    messages: list[Message] = []
    stats = ParseStats()
    cwd, sid = "", path.stem
    thread_source, parent_thread_id, agent_role = "user", None, None
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            payload = obj.get("payload") or {}
            if not isinstance(payload, dict):
                continue
            source = payload.get("source") if isinstance(payload.get("source"), dict) else obj.get("source")
            if isinstance(source, dict):
                spawned = source.get("subagent", {}).get("thread_spawn") if isinstance(source.get("subagent"), dict) else None
                parent = spawned
                if isinstance(spawned, dict):
                    parent = next(
                        (spawned.get(key) for key in ("parent_thread_id", "parentThreadId", "id")
                         if isinstance(spawned.get(key), (str, int)) and not isinstance(spawned.get(key), bool)),
                        None,
                    )
                if parent:
                    thread_source = "subagent"
                    parent_thread_id = str(parent)
                    turns.clear()
            if obj.get("type") == "session_meta":
                if (payload.get("originator") == "codex_exec"
                        and payload.get("source") == "exec"):
                    return None
                cwd = payload.get("cwd") or cwd
                sid = payload.get("id") or payload.get("session_id") or sid
                agent_role = payload.get("agent_role") or payload.get("role_name") or agent_role
                continue
            role = payload.get("role")
            agent_role = payload.get("agent_role") or payload.get("role_name") or agent_role
            if obj.get("type") == "event_msg" and payload.get("type") == "agent_message":
                role = "agent"
            if obj.get("type") not in {"response_item", "event_msg"} or role not in {"user", "developer", "assistant", "agent"}:
                if obj.get("type") not in ("response_item", "session_meta", "event_msg"):
                    stats.unknown_rows += 1
                continue
            blocks = payload.get("content") or []
            if not blocks and isinstance(payload.get("message"), str):
                blocks = [{"text": payload["message"]}]
            for item in blocks:
                if isinstance(item, dict) and isinstance(item.get("text"), str):
                    text = item["text"].strip()
                    if text:
                        messages.append(Message(text, scope_for_cwd(cwd), str(role),
                                                obj.get("timestamp") if isinstance(obj.get("timestamp"), str) else None,
                                                payload.get("author"), payload.get("recipient")))
                    if role == "user" and _keep_turn(text) and thread_source == "user":
                        cleaned = _clean(text, stats)
                        if cleaned is not None:
                            turns.append(Turn(
                                cleaned,
                                scope_for_cwd(cwd),
                                obj.get("timestamp") if isinstance(obj.get("timestamp"), str) else None,
                            ))
                            stats.kept_turns += 1
                    elif role == "user":
                        stats.dropped_turns += 1
            if max_turns is not None and len(turns) >= max_turns:
                stats.dropped_turns += 1
                break
    if not messages:
        return None
    return Session("codex", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   messages, agent_role, thread_source, parent_thread_id)


def _text_blocks(content: object) -> list[str]:
    if isinstance(content, str):
        return [content]
    if not isinstance(content, list):
        return []
    return [
        str(block.get("text") or "")
        for block in content
        if isinstance(block, dict) and block.get("type") in {None, "text"}
        and isinstance(block.get("text"), str)
    ]


def _unwrap_external_user_text(text: str) -> str:
    for _ in range(3):
        match = re.fullmatch(
            r"\s*<user_input(?:\s+[^>]{1,200})?>([\s\S]*?)</user_input>\s*",
            text,
        )
        if match is None:
            break
        text = match.group(1).strip()
    return text


def _normalize_observed_at(value: object) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        seconds = value / 1000 if value > 10_000_000_000 else value
        try:
            return datetime.fromtimestamp(seconds, timezone.utc).isoformat().replace(
                "+00:00", "Z"
            )
        except (OverflowError, OSError, ValueError):
            return None
    return None


def _append_user_texts(
    turns: list[Turn], stats: ParseStats, content: object, cwd: str,
    observed_at: object = None, *, max_turns: int | None,
) -> bool:
    for raw in _text_blocks(content):
        text = _unwrap_external_user_text(raw.strip())
        if _keep_turn(text):
            turns.append(Turn(
                _clean(text, stats), scope_for_cwd(cwd),
                _normalize_observed_at(observed_at),
            ))
            stats.kept_turns += 1
        else:
            stats.dropped_turns += 1
        if max_turns is not None and len(turns) >= max_turns:
            stats.dropped_turns += 1
            return True
    return False


def parse_command_code_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
    turns: list[Turn] = []
    stats = ParseStats()
    sid = path.stem
    cwd = path.parent.name
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                stats.unknown_rows += 1
                continue
            if not isinstance(row, dict) or row.get("role") != "user":
                continue
            sid = str(row.get("sessionId") or sid)
            if _append_user_texts(
                turns, stats, row.get("content"), cwd, row.get("timestamp"),
                max_turns=max_turns,
            ):
                break
    return Session("command-code", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   _messages_for_turns(turns)) if turns else None


# Compatibility for the original adapter API used by local callers and tests.
parse_commandcode_session = parse_command_code_session


def parse_cline_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
    body = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    if not isinstance(body, dict) or not isinstance(body.get("messages"), list):
        raise ValueError("invalid Cline session")
    companion = path.with_name(path.name.removesuffix(".messages.json") + ".json")
    metadata: dict = {}
    if companion.is_file():
        loaded = json.loads(companion.read_text(encoding="utf-8", errors="replace"))
        if isinstance(loaded, dict):
            metadata = loaded
    sid = str(body.get("sessionId") or metadata.get("session_id") or path.parent.name)
    cwd = str(metadata.get("workspace_root") or metadata.get("cwd") or path.parent.parent.name)
    turns: list[Turn] = []
    stats = ParseStats()
    prompt = metadata.get("prompt")
    if isinstance(prompt, str) and prompt.strip():
        if _append_user_texts(
            turns, stats, prompt, cwd, metadata.get("started_at"), max_turns=max_turns
        ):
            return Session("cline", sid, path, cwd, path.stat().st_mtime, turns, stats,
                           _messages_for_turns(turns))
    for row in body["messages"]:
        if not isinstance(row, dict) or row.get("role") != "user":
            continue
        if _append_user_texts(
            turns, stats, row.get("content"), cwd, row.get("ts"), max_turns=max_turns
        ):
            break
    return Session("cline", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   _messages_for_turns(turns)) if turns else None


def parse_gemini_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
    rows: list[dict] = []
    if path.suffix == ".jsonl":
        with path.open(encoding="utf-8", errors="replace") as handle:
            for line in handle:
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(row, dict):
                    rows.append(row)
    else:
        body = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        if isinstance(body, list):
            rows = [row for row in body if isinstance(row, dict)]
        elif isinstance(body, dict):
            messages = body.get("messages") or body.get("history") or []
            rows = [row for row in messages if isinstance(row, dict)]
            rows.insert(0, body)
        else:
            raise ValueError("invalid Gemini session")
    sid = path.stem
    cwd = path.parent.parent.name
    turns: list[Turn] = []
    stats = ParseStats()
    for row in rows:
        sid = str(row.get("sessionId") or sid)
        if row.get("type") != "user" and row.get("role") not in {"user", "human"}:
            continue
        if _append_user_texts(
            turns, stats, row.get("content"), cwd, row.get("timestamp"),
            max_turns=max_turns,
        ):
            break
    return Session("gemini", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   _messages_for_turns(turns)) if turns else None


def parse_grok_build_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
    sid = path.parent.name
    cwd = urllib.parse.unquote(path.parent.parent.name)
    turns: list[Turn] = []
    stats = ParseStats()
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                stats.unknown_rows += 1
                continue
            if (
                not isinstance(row, dict) or row.get("type") != "user"
                or row.get("synthetic_reason")
            ):
                continue
            if _append_user_texts(
                turns, stats, row.get("content"), cwd, row.get("timestamp"),
                max_turns=max_turns,
            ):
                break
    return Session("grok-build", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   _messages_for_turns(turns)) if turns else None


def parse_roo_cline_session(
    path: Path, *, max_turns: int | None = MAX_TURNS_PER_SESSION
) -> Session | None:
    body = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    if not isinstance(body, list):
        raise ValueError("invalid Roo-Cline session")
    sid = path.parent.name
    cwd = "cursor-roo-cline"
    turns: list[Turn] = []
    stats = ParseStats()
    for row in body:
        if not isinstance(row, dict) or row.get("role") != "user":
            continue
        if _append_user_texts(
            turns, stats, row.get("content"), cwd, row.get("ts"), max_turns=max_turns
        ):
            break
    return Session("roo-cline", sid, path, cwd, path.stat().st_mtime, turns, stats,
                   _messages_for_turns(turns)) if turns else None


PARSERS = {
    "claude-code": "parse_claude_session",
    "codex": "parse_codex_session",
    "command-code": "parse_command_code_session",
    "cline": "parse_cline_session",
    "gemini": "parse_gemini_session",
    "grok-build": "parse_grok_build_session",
    "roo-cline": "parse_roo_cline_session",
}


def parser_for(tool: str):
    try:
        return globals()[PARSERS[tool]]
    except KeyError as exc:
        raise ValueError(f"unsupported transcript tool: {tool}") from exc


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
    command_root = Path.home() / ".commandcode" / "projects"
    if command_root.exists():
        found.extend(
            ("command-code", path)
            for path in sorted(command_root.glob("*/*.jsonl"))
            if not path.name.endswith(".checkpoints.jsonl")
        )
    cline_root = Path.home() / ".cline" / "data" / "sessions"
    if cline_root.exists():
        found.extend(
            ("cline", path)
            for path in sorted(cline_root.glob("*/*.messages.json"))
            if path.name == f"{path.parent.name}.messages.json"
        )
    gemini_root = Path.home() / ".gemini" / "tmp"
    if gemini_root.exists():
        found.extend(
            ("gemini", path)
            for path in sorted(gemini_root.glob("*/chats/session-*.json*"))
            if path.suffix in {".json", ".jsonl"}
        )
    grok_root = Path.home() / ".grok" / "sessions"
    if grok_root.exists():
        found.extend(
            ("grok-build", path)
            for path in sorted(grok_root.glob("*/*/chat_history.jsonl"))
        )
    roo_roots = (
        Path.home() / "AppData" / "Roaming" / "Cursor" / "User" / "globalStorage"
        / "rooveterinaryinc.roo-cline" / "tasks",
        Path.home() / "Library" / "Application Support" / "Cursor" / "User"
        / "globalStorage" / "rooveterinaryinc.roo-cline" / "tasks",
    )
    for roo_root in roo_roots:
        if roo_root.exists():
            found.extend(
                ("roo-cline", path)
                for path in sorted(roo_root.glob("*/api_conversation_history.json"))
            )
    return found


def state_key(tool: str, path: Path) -> str:
    if tool == "cline" and path.name.endswith(".messages.json"):
        return path.name.removesuffix(".messages.json")
    if tool == "grok-build":
        return path.parent.name
    if tool == "roo-cline":
        return path.parent.name
    return path.stem


def is_active_session(
    tool: str, path: Path, *, env: Mapping[str, str] = os.environ
) -> bool:
    """Return true for a Codex transcript known to still be writing."""
    active_ids = {
        value.strip()
        for value in (
            env.get("CODEX_THREAD_ID", ""),
            *env.get("MORPH_ACTIVE_CODEX_THREAD_IDS", "").split(","),
        )
        if value.strip()
    }
    return tool == "codex" and any(value in path.stem for value in active_ids)


def new_sessions(
    state: dict,
    limit: int | None = None,
    *,
    newest: bool = False,
    before_mtime: float | None = None,
) -> list[Session]:
    """Parse sessions not yet learned (or modified since). Excluded/empty ones are
    marked learned immediately so they are never re-parsed."""
    learned = state.setdefault("learned", {})
    out: list[Session] = []
    pending: list[tuple[float, str, Path]] = []
    for tool, path in discover():
        if is_active_session(tool, path):
            continue
        key = state_key(tool, path)
        mtime = path.stat().st_mtime
        if before_mtime is not None and mtime > before_mtime:
            continue
        if learned.get(tool, {}).get(key, -1.0) >= mtime:
            continue
        pending.append((mtime, tool, path))
    pending.sort(key=lambda item: item[0], reverse=newest)
    for mtime, tool, path in pending:
        key = state_key(tool, path)
        sess = parser_for(tool)(path)
        if sess is None or scope_excluded(sess.cwd) or (sess.thread_source != "user" and not sess.turns):
            learned.setdefault(tool, {})[key] = mtime
            continue
        out.append(sess)
        if limit is not None and len(out) >= limit:
            break
    out.sort(key=lambda s: s.mtime)
    return out


def mark_learned(state: dict, sessions: list[Session]) -> None:
    for s in sessions:
        state.setdefault("learned", {}).setdefault(s.tool, {})[
            state_key(s.tool, s.path)
        ] = s.mtime
