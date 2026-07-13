"""Deterministic authority snapshot and candidate safety checks for Adapt.

The manifest freezes source identities and hashes; the source files remain the
authority. Adapt uses this module to quarantine mechanically detectable
conflicts before any candidate can become an instruction.
"""
from __future__ import annotations

import dataclasses
import fnmatch
import hashlib
import json
import re
from pathlib import Path
from typing import Iterable

MANIFEST_SCHEMA_VERSION = "1.0.0"
RECORD_TYPES = frozenset({
    "standing_preference",
    "locked_decision",
    "operational_playbook",
    "episodic_fact",
    "unclassified",
})
AUTHORITY_EFFECTS = frozenset({
    "neutral",
    "restrictive",
    "permission_expanding",
})

_WS_RE = re.compile(r"\s+")
_MARKDOWN_PREFIX_RE = re.compile(r"^(?:#{1,6}\s+|[-*+]\s+|\d+[.)]\s+)")
_LEADING_MODAL_RE = re.compile(
    r"^(?:always|never|do not|don't|must not|must|should not|should|only)\s+"
)
_RESTRICTIVE_RE = re.compile(
    r"^(?:never\b|do not\b|don't\b|must not\b|should not\b)|"
    r"\b(?:requires?|only after|only with) explicit (?:user )?(?:approval|permission|review)\b"
)
_PERMISSION_PATTERNS = (
    re.compile(r"\btreat\b.+\bas (?:implicitly )?authorized\b"),
    re.compile(r"\bauthorized unless\b"),
    re.compile(r"\bwithout (?:explicit )?(?:approval|permission|review)\b"),
    re.compile(r"\b(?:skip|bypass|disable)\b.+\b(?:approval|review|gate|scanner|security)\b"),
    re.compile(r"\b(?:edit|modify|deploy)\b.+\bproduction\b.+\b(?:directly|ssh)\b"),
    re.compile(r"\b(?:may|can)\b.+\bwithout (?:approval|permission|review)\b"),
)


@dataclasses.dataclass(frozen=True)
class AuthorityResult:
    admitted: bool
    reason: str
    authority_effect: str


def normalize_text(text: str) -> str:
    value = _MARKDOWN_PREFIX_RE.sub("", (text or "").strip())
    value = value.lower().replace("`", "")
    value = re.sub(r"[.!?,;:]+$", "", value)
    return _WS_RE.sub(" ", value).strip()


def normalize_record_type(value: str | None) -> str:
    normalized = (value or "unclassified").strip().lower()
    return normalized if normalized in RECORD_TYPES else "unclassified"


def classify_authority_effect(text: str) -> str:
    normalized = normalize_text(text)
    if _RESTRICTIVE_RE.search(normalized):
        return "restrictive"
    if any(pattern.search(normalized) for pattern in _PERMISSION_PATTERNS):
        return "permission_expanding"
    return "neutral"


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _manifest_hash(manifest: dict) -> str:
    payload = {k: v for k, v in manifest.items() if k != "manifest_sha256"}
    encoded = json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return _sha256_bytes(encoded)


def discover_sources(root: Path, extra_paths: Iterable[Path] = ()) -> list[Path]:
    root = root.resolve()
    candidates = [root / "AGENTS.md", root / "CLAUDE.md"]
    rules_dir = root / ".claude" / "rules"
    if rules_dir.exists():
        candidates.extend(rules_dir.rglob("*.md"))
    candidates.extend(Path(path) for path in extra_paths)
    unique: dict[str, Path] = {}
    for path in candidates:
        resolved = path if path.is_absolute() else root / path
        resolved = resolved.resolve()
        if resolved.is_file():
            try:
                relative = resolved.relative_to(root).as_posix()
            except ValueError as exc:
                raise ValueError(f"authority source escapes root: {resolved}") from exc
            unique[relative] = resolved
    return [unique[key] for key in sorted(unique)]


def build_manifest(
    root: Path,
    *,
    extra_paths: Iterable[Path] = (),
    directives: Iterable[dict] = (),
    forbidden_scopes: Iterable[str] = (),
) -> dict:
    """Build a reproducible, content-hashed snapshot of authority sources."""
    root = root.resolve()
    sources = []
    for path in discover_sources(root, extra_paths):
        raw = path.read_bytes()
        relative = path.relative_to(root).as_posix()
        sources.append({
            "path": relative,
            "scope": "workspace",
            "rank": 100,
            "bytes": len(raw),
            "sha256": _sha256_bytes(raw),
        })
    normalized_directives = []
    for item in directives:
        normalized_directives.append({
            "id": str(item["id"]),
            "text": str(item["text"]),
            "scope": str(item.get("scope", "workspace")),
            "status": str(item.get("status", "current")),
        })
    normalized_directives.sort(key=lambda item: item["id"])
    manifest = {
        "schema_version": MANIFEST_SCHEMA_VERSION,
        "sources": sources,
        "directives": normalized_directives,
        "forbidden_scopes": sorted(set(str(scope) for scope in forbidden_scopes)),
    }
    manifest["manifest_sha256"] = _manifest_hash(manifest)
    return manifest


def write_manifest(manifest: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")


def verify_manifest(manifest: dict, root: Path) -> list[str]:
    """Return all integrity errors. An empty list means the snapshot is valid."""
    errors: list[str] = []
    if manifest.get("schema_version") != MANIFEST_SCHEMA_VERSION:
        errors.append("unsupported authority manifest schema")
    if manifest.get("manifest_sha256") != _manifest_hash(manifest):
        errors.append("authority manifest hash mismatch")
    root = root.resolve()
    for source in manifest.get("sources", []):
        relative = str(source.get("path", ""))
        path = (root / relative).resolve()
        try:
            path.relative_to(root)
        except ValueError:
            errors.append(f"authority source escapes root: {relative}")
            continue
        if not path.is_file():
            errors.append(f"authority source missing: {relative}")
            continue
        if _sha256_bytes(path.read_bytes()) != source.get("sha256"):
            errors.append(f"authority source hash mismatch: {relative}")
    return errors


def _literal_signature(text: str) -> str:
    normalized = normalize_text(text)
    return _LEADING_MODAL_RE.sub("", normalized).strip()


def _scope_applies(authority_scope: str, candidate_scope: str) -> bool:
    authority_scope = (authority_scope or "workspace").strip()
    if authority_scope in {"workspace", "global", "*"}:
        return True
    if fnmatch.fnmatchcase(candidate_scope, authority_scope):
        return True
    return candidate_scope.startswith(f"{authority_scope}/")


def _authority_lines(manifest: dict, root: Path, scope: str) -> list[str]:
    lines: list[str] = []
    for source in manifest.get("sources", []):
        if not _scope_applies(source.get("scope", "workspace"), scope):
            continue
        path = root.resolve() / source["path"]
        lines.extend(path.read_text(encoding="utf-8", errors="replace").splitlines())
    lines.extend(
        item.get("text", "")
        for item in manifest.get("directives", [])
        if item.get("status", "current") == "current"
        and _scope_applies(item.get("scope", "workspace"), scope)
    )
    return [line for line in lines if normalize_text(line)]


def evaluate_rule(
    rule: str,
    *,
    scope: str,
    declared_effect: str | None = None,
    authority_manifest: dict | None = None,
    authority_root: Path | None = None,
) -> AuthorityResult:
    """Evaluate one candidate, quarantining deterministic safety failures."""
    computed_effect = classify_authority_effect(rule)
    effect = computed_effect
    if declared_effect == "permission_expanding":
        effect = "permission_expanding"
    elif declared_effect == "restrictive" and computed_effect == "neutral":
        effect = "restrictive"
    if effect == "permission_expanding":
        return AuthorityResult(False, "permission-expanding", effect)
    if authority_manifest is None:
        return AuthorityResult(True, "ok", effect)
    if authority_root is None:
        return AuthorityResult(False, "authority-manifest-invalid", effect)
    if verify_manifest(authority_manifest, authority_root):
        return AuthorityResult(False, "authority-manifest-invalid", effect)

    for pattern in authority_manifest.get("forbidden_scopes", []):
        if fnmatch.fnmatchcase(scope, pattern):
            return AuthorityResult(False, "forbidden-scope", effect)

    normalized_rule = normalize_text(rule)
    for directive in authority_manifest.get("directives", []):
        if directive.get("status") != "superseded":
            continue
        if not _scope_applies(directive.get("scope", "workspace"), scope):
            continue
        if normalize_text(directive.get("text", "")) == normalized_rule:
            return AuthorityResult(False, "superseded-decision", effect)

    candidate_signature = _literal_signature(rule)
    candidate_restrictive = classify_authority_effect(rule) == "restrictive"
    for line in _authority_lines(authority_manifest, authority_root, scope):
        if _literal_signature(line) != candidate_signature:
            continue
        authority_restrictive = classify_authority_effect(line) == "restrictive"
        if authority_restrictive != candidate_restrictive:
            return AuthorityResult(False, "authority-conflict", effect)
    return AuthorityResult(True, "ok", effect)
