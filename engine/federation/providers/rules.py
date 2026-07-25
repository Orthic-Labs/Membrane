"""Rules provider — emits AGENTS.md, .claude/rules/*.md, .codex/rules/*.md as candidates.

Each rule file is byte-capped (default 1500) with a TRUNCATED marker.
"""
from __future__ import annotations

from pathlib import Path

CAP_BYTES = 1500


def produce(repo_root: Path, task: str) -> list[dict]:
    """Read repo-scoped rule files. Cap each at CAP_BYTES with a marker."""
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
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:
            raise RuntimeError(f"read {path}: {exc}")
        truncated = text[:CAP_BYTES]
        if len(text) > CAP_BYTES:
            truncated = truncated + "\n[TRUNCATED]"
        rel = str(path.relative_to(repo_root))
        candidates.append({
            "id": f"rules:{rel}",
            "layer": 2,
            "sourceKind": "doc",
            "sourceRef": rel,
            "sourceHash": "0" * 64,
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": 0.3,
            "scoreComponents": {"rule_relevance": 0.3},
            "estimatedTokens": max(1, len(truncated) // 4),
            "protected": False,
            "exact": False,
            "recoverable": True,
            "resolver": f"read {rel}",
            "text": truncated,
        })
    return candidates
