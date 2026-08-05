"""Rules provider — emits AGENTS.md, .claude/rules/*.md, .codex/rules/*.md as candidates.

Each rule file is delivered in one of three modes (plan 2.3/2.4):
  - ``native``   — the host already loaded the file itself; never serialize it.
  - ``inline``   — headless client, content unchanged since last delivery.
  - ``reference`` — headless client, unchanged since last delivery, deliver only a resolver.

Self-loading clients (hosts that already load workspace rule files at
session start) get reference-only candidates with ``deliveryMode="native"``,
since inlining would just be a truncated duplicate of what the client already
has. Headless clients get the full policy once per session, then zero static
bytes until the sourceHash changes — implemented via a per-session delivery
ledger that persists across the gateway's per-prompt cold Python spawns.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path

# Plan 2.4: the headless path ships the FULL static policy in one go; nothing
# bulk rides the prompt after that. The historical 1,500-byte cap was the
# defect — it truncated policies to fit a budget that the new model
# eliminates. Kept only as a last-resort safety net for pathological files
# (well over a megabyte) so a single rogue workspace cannot bloat the prompt.
CAP_BYTES = 1_500_000  # 1.5 MB hard ceiling on inline text; never truncates normal policies.

# Plan convention 3: the one typed client identity, shared by every adapter and
# telemetry surface. Adapters emit exactly these strings (see
# forge/hooks/membrane-context.js and hooks/claude-code/tool-receipt.js);
# anything else degrades to "other" at the adapter rather than inventing a new
# name here.
CLIENT_IDENTITIES = frozenset({"claude_code", "codex", "mcp", "api_worker", "other"})

# Clients whose HOST already loads the workspace rule files (AGENTS.md,
# .claude/rules/*.md, .codex/rules/*.md) at session start on its own —
# e.g. Claude Code imports CLAUDE.md which chains into workspace rules, and
# Codex natively walks the AGENTS.md chain. For these clients, inlining rule
# text in the packet is a redundant (and truncated) duplicate of content the
# client already has in full. Any client not in this set is assumed NOT to
# self-load, so it gets the full content — correctness over token savings.
#
# "claude" is the retired pre-convention-3 spelling, kept only so a rolling
# upgrade (new engine, not-yet-updated adapter) does not regress to inlining.
SELF_LOADING_RULE_CLIENTS = frozenset({"claude", "claude_code", "codex"})

# Typed delivery modes stamped on every candidate (plan 2.3).
DELIVERY_MODES = frozenset({"native", "inline", "reference"})

# The ledger is a single JSON file under tools/.cache/. Writing is best-effort:
# if the cache directory is unwritable (e.g. read-only install) the provider
# degrades to "always inline", which is the same behavior as if no ledger
# existed. The ledger is the per-session delivery memory; losing it means the
# headless client re-receives the full policy on the next turn — a correctness
# tax, not a regression.
_WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
_LEDGER_PATH = _WORKSPACE_ROOT / "tools" / ".cache" / "membrane-rules-ledger.json"


def _sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _ledger_load() -> dict:
    """Load the persistent ledger; return empty dict on any failure."""
    try:
        raw = _LEDGER_PATH.read_bytes()
    except OSError:
        return {}
    if not raw:
        return {}
    try:
        parsed = json.loads(raw.decode("utf-8", errors="replace"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return {}
    return parsed if isinstance(parsed, dict) else {}


def _ledger_save(ledger: dict) -> None:
    """Persist the ledger; best-effort, never raises."""
    try:
        _LEDGER_PATH.parent.mkdir(parents=True, exist_ok=True)
        tmp = _LEDGER_PATH.with_suffix(_LEDGER_PATH.suffix + ".tmp")
        tmp.write_text(json.dumps(ledger, ensure_ascii=False, sort_keys=True), encoding="utf-8")
        os.replace(tmp, _LEDGER_PATH)
    except OSError:
        # Read-only install / locked-down permissions. Degrade to "always
        # inline" rather than failing the prompt.
        pass


def _session_key(repo_root: Path, client: str, session: str) -> str:
    """One ledger key per (repo, client, session) triple.

    A real gateway passes ``session`` through; tests and CLI invocations that
    omit it collapse to one bucket per (repo, client) — which is exactly what
    E2E-4 needs (three sequential ``R.produce`` calls in one test = one
    session).
    """
    safe_session = (session or "").strip() or f"anon:{repo_root.resolve()}"
    return f"{repo_root.resolve()}|{client}|{safe_session}"


def produce(
    repo_root: Path,
    task: str,
    client: str = "",
    session: str = "",
) -> list[dict]:
    """Read repo-scoped rule files; stamp ``deliveryMode`` per plan 2.3/2.4.

    In native mode (self-loading clients), each candidate carries no inlined
    text and ``deliveryMode="native"``. In headless mode, the first turn for
    a given (repo, client, session, sourceRef) ships the full policy with
    ``deliveryMode="inline"``; subsequent turns with the same sourceHash ship
    ``text=""`` and ``deliveryMode="reference"``; a changed sourceHash
    re-delivers inline.

    The ledger persists at ``tools/.cache/membrane-rules-ledger.json`` so the
    per-prompt gateway cold-spawn does not reset the session's delivery
    memory.
    """
    self_loading = client in SELF_LOADING_RULE_CLIENTS

    rule_paths: list[Path] = []
    for rel in ("AGENTS.md", ".claude/rules", ".codex/rules"):
        p = repo_root / rel
        if p.is_file():
            rule_paths.append(p)
        elif p.is_dir():
            for child in sorted(p.rglob("*.md")):
                if child.is_file():
                    rule_paths.append(child)
    candidates: list[dict] = []
    # Native candidates never touch the ledger — a self-loading host is
    # recorded as delivered-by-host on every turn regardless of sourceHash.
    ledger = _ledger_load() if not self_loading else {}
    ledger_dirty = False
    for path in rule_paths:
        try:
            raw = path.read_bytes()
        except OSError as exc:
            raise RuntimeError(f"read {path}: {exc}")
        text = raw.decode("utf-8", errors="replace")
        source_hash = _sha256_hex(raw)
        rel = str(path.relative_to(repo_root))
        resolver = f"read {rel}"

        if self_loading:
            body = ""
            estimated_tokens = max(1, len(resolver) // 4)
            delivery_mode = "native"
            truncated_flag = False
        else:
            # Headless client: deliver the FULL policy once, then references
            # until the file changes. The hard ceiling exists only to keep a
            # pathological workspace from inflating the prompt — normal
            # policies never approach it.
            skey = _session_key(repo_root, client, session)
            slot_key = f"{skey}|{rel}"
            previous_hash = ledger.get(slot_key)
            if previous_hash == source_hash:
                body = ""
                estimated_tokens = max(1, len(resolver) // 4)
                delivery_mode = "reference"
                truncated_flag = False
            else:
                body = text
                if len(body) > CAP_BYTES:
                    body = body[:CAP_BYTES] + "\n[TRUNCATED]"
                    truncated_flag = True
                else:
                    truncated_flag = False
                estimated_tokens = max(1, len(body) // 4)
                delivery_mode = "inline"
                ledger[slot_key] = source_hash
                ledger_dirty = True

        candidates.append({
            "id": f"rules:{rel}",
            "layer": 2,
            "sourceKind": "doc",
            "sourceRef": rel,
            "sourceHash": source_hash,
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": 0.3,
            "scoreComponents": {"rule_relevance": 0.3},
            "estimatedTokens": estimated_tokens,
            "protected": False,
            "exact": False,
            "recoverable": True,
            "resolver": resolver,
            "truncated": truncated_flag,
            "text": body,
            # Plan 2.3: per-candidate delivery-mode stamp so the ledger is
            # observable from the candidate itself, not just the session
            # record.
            "deliveryMode": delivery_mode,
        })

    if ledger_dirty:
        _ledger_save(ledger)
    return candidates