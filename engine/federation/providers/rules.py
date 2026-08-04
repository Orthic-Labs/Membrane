"""Rules provider — emits AGENTS.md, .claude/rules/*.md, .codex/rules/*.md as candidates.

Each rule file is byte-capped (default 1500) with a TRUNCATED marker when
emitted in content mode. Self-loading clients (hosts that already load
workspace rule files at session start) get reference-only candidates instead,
since inlining would just be a truncated duplicate of what the client already
has.
"""
from __future__ import annotations

import hashlib
from pathlib import Path

CAP_BYTES = 1500

# Clients whose HOST already loads the workspace rule files (AGENTS.md,
# .claude/rules/*.md, .codex/rules/*.md) at session start on its own —
# e.g. Claude Code imports CLAUDE.md which chains into workspace rules, and
# Codex natively walks the AGENTS.md chain. For these clients, inlining rule
# text in the packet is a redundant (and truncated) duplicate of content the
# client already has in full. Any client not in this set is assumed NOT to
# self-load, so it gets the full content — correctness over token savings.
SELF_LOADING_RULE_CLIENTS = frozenset({"claude", "claude_code", "codex"})


def _sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def produce(repo_root: Path, task: str, client: str = "") -> list[dict]:
    """Read repo-scoped rule files.

    In content mode (default, and for any client not known to self-load
    workspace rules), each file is capped at CAP_BYTES with a marker. In
    reference mode (self-loading clients), candidates carry no inlined text
    and instead point at the resolver so the consumer can fetch on demand.
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
            truncated_flag = False
            estimated_tokens = max(1, len(resolver) // 4)
        else:
            body = text[:CAP_BYTES]
            truncated_flag = len(text) > CAP_BYTES
            if truncated_flag:
                body = body + "\n[TRUNCATED]"
            estimated_tokens = max(1, len(body) // 4)

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
        })
    return candidates
