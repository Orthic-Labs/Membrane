"""Deterministic authority snapshot and candidate safety checks for Morph.

The manifest freezes source identities and hashes; the source files remain the
authority. Morph uses this module to quarantine mechanically detectable
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
    "security_weakening",
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
# Insecure *coding taste* — a distinct class from permission expansion. A rule can leave every
# approval gate intact and still teach the agent to write unsafe code. Mined preferences are the
# wrong place for these regardless of how often a transcript appears to endorse them.
_INSECURE_PATTERNS = (
    # transport / certificate verification
    re.compile(r"\b(?:disable|skip|turn off|ignore|bypass)\b.{0,40}\b"
               r"(?:tls|ssl|https|certificate|cert)\b.{0,20}\b(?:verif\w*|validat\w*|check\w*)\b"),
    # verb-before-noun and negated forms: "never validate certificates", "don't verify TLS"
    re.compile(r"\b(?:never|do not|don't|no need to|stop)\b.{0,20}"
               r"\b(?:verif\w*|validat\w*|check\w*)\b.{0,20}"
               r"\b(?:tls|ssl|https|certificate|cert|signature|checksum|hash)\w*\b"),
    re.compile(r"\b(?:verify\s*=\s*false|rejectunauthorized\s*:?\s*false|insecureskipverify)\b"),
    # NOTE: no leading \b before a hyphen-flag — space→"-" is not a word boundary, so "\b-k" never
    # matches. Anchor on whitespace/start instead.
    re.compile(r"\bcurl\b.{0,20}(?:^|\s)(?:-k|--insecure)\b"),
    # credential handling
    re.compile(r"\b(?:hardcode|hard-code|inline|embed|commit)\b.{0,30}"
               r"\b(?:secret|credential|password|api[- ]?key|token|private key)s?\b"),
    re.compile(r"\b(?:secret|credential|password|api[- ]?key|token)s?\b.{0,30}"
               r"\bin(?:to)?\b.{0,20}\b(?:source|repo|git|code|argv|command line|log)s?\b"),
    # crypto downgrade
    re.compile(r"\b(?:use|prefer|switch to)\b.{0,20}\b(?:md5|sha1|des|rc4|ecb)\b"),
    re.compile(r"\b(?:weaken|lower|reduce)\b.{0,30}\b(?:crypto\w*|encryption|hashing|key length)\b"),
    # validation / sanitization removal
    re.compile(r"\b(?:disable|skip|remove|drop|turn off)\b.{0,40}"
               r"\b(?:input validation|sanitiz\w*|escap\w*|csrf|cors|auth\w*|authoriz\w*|permission check)\b"),
    re.compile(r"\b(?:raw|unparameterized|string[- ]concatenated)\b.{0,20}\bsql\b"),
    re.compile(r"\b(?:eval|exec)\b.{0,30}\buser\b.{0,20}\binput\b"),
    # suppressing the tools that would catch the above
    re.compile(r"\b(?:disable|skip|suppress|ignore|delete|remove)\b.{0,40}"
               r"\b(?:test|assertion|lint\w*|type ?check\w*|security scan\w*|audit)\w*\b"),
    re.compile(r"(?:^|\s)--no-verify\b|\b(?:nosec|noqa\b.{0,10}s\d|eslint-disable\b.{0,30}security)\b"),
)


# ----- AD9: origin-authority boundary -----
#
# Only an authenticated USER turn may establish standing preference authority.
# Repository file content, tool output echoed back into a session, and
# assistant-authored text must never be able to create a durable rule — that
# is exactly the memory-poisoning / prompt-injection vector (a crafted
# CLAUDE.md, a tool result, or the assistant's own narration gets mined as if
# Adrian said it). This extends the same deterministic-quarantine approach as
# `_PERMISSION_PATTERNS`/`_INSECURE_PATTERNS` above: explicit origin tagging
# is checked first, then a lexical fallback flags content that LOOKS like an
# echoed file/tool/assistant artifact even when mistagged as user_turn.
ORIGIN_VALUES: frozenset[str] = frozenset({
    "user_turn", "assistant_output", "tool_output", "repo_file", "unknown",
})
_NON_USER_ORIGINS: frozenset[str] = frozenset({
    "assistant_output", "tool_output", "repo_file",
})

_REPO_FILE_ECHO_PATTERNS = (
    # Read-tool / `cat -n` style line-numbered output, or a pasted SKILL.md /
    # CLAUDE.md front-matter block.
    re.compile(r"(?m)^\s*\d+\t"),
    re.compile(r"(?m)^---\s*$[\s\S]{0,200}?^(?:name|description)\s*:", re.MULTILINE),
    re.compile(r"(?i)\bcontents? of\b.{0,80}\.(?:md|py|json|ya?ml|txt)\b"),
    re.compile(r"(?im)^#\s+(?:CLAUDE|AGENTS)\.md\b"),
)
_TOOL_OUTPUT_ECHO_PATTERNS = (
    re.compile(r'"tool_use_id"\s*:'),
    re.compile(r'"is_error"\s*:'),
    re.compile(r"(?m)^\$\s+\S"),
    re.compile(r"(?i)\b(?:stdout|stderr)\b\s*:"),
    re.compile(r"(?i)<tool_result>|<function_results>"),
)
_ASSISTANT_AUTHORED_PATTERNS = (
    re.compile(r"(?i)^(?:i'll|i will|let me|certainly!?|sure,? i(?:'ll| will))\b"),
    re.compile(r"(?i)\bas (?:claude|the assistant|an ai)\b"),
    re.compile(r"(?i)\bi(?:'ve| have) (?:implemented|added|fixed|updated|created)\b"),
)


def classify_content_origin_hint(text: str) -> str | None:
    """Lexical best-effort signal that `text` is echoed repo/tool/assistant
    content rather than hand-typed user text. Returns 'tool_output',
    'repo_file', or 'assistant_output', or None when nothing fires (treated
    as ordinary user content)."""
    body = text or ""
    if any(p.search(body) for p in _TOOL_OUTPUT_ECHO_PATTERNS):
        return "tool_output"
    if any(p.search(body) for p in _REPO_FILE_ECHO_PATTERNS):
        return "repo_file"
    if any(p.search(body) for p in _ASSISTANT_AUTHORED_PATTERNS):
        return "assistant_output"
    return None


def evaluate_origin(origin: str | None, evidence_text: str = "") -> "AuthorityResult":
    """Refuse to let anything but an authenticated user turn establish
    authority. `origin`, if given, must be one of ORIGIN_VALUES; an explicit
    non-user origin refuses immediately. Regardless of the declared origin,
    the lexical fallback still scans `evidence_text` — a mistagged or
    untagged turn that merely *echoes* repo/tool/assistant content is refused
    too, since the injection risk lives in the content, not the label."""
    resolved = (origin or "user_turn").strip().lower()
    if resolved not in ORIGIN_VALUES:
        resolved = "unknown"
    if resolved in _NON_USER_ORIGINS:
        return AuthorityResult(False, f"origin-not-user:{resolved}", "neutral")
    hint = classify_content_origin_hint(evidence_text)
    if hint is not None:
        return AuthorityResult(False, f"origin-not-user:{hint}", "neutral")
    return AuthorityResult(True, "ok", "neutral")


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
    # Security-weakening is checked BEFORE restrictive: "never validate certificates" reads as
    # restrictive by surface form ("never ...") while being exactly the rule we must refuse.
    if any(pattern.search(normalized) for pattern in _INSECURE_PATTERNS):
        return "security_weakening"
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


# Conflict detection compares POLARITY, not authority effect. Reusing
# `classify_authority_effect(...) == "restrictive"` for this was a real bug: an
# authority line like "Never skip tests." classifies as security_weakening (the
# insecure-pattern set matches "skip ... test" by design), never as restrictive.
# Both sides of the comparison then collapsed to the same value, so a genuine
# contradiction was invisible for every security-adjacent topic -- exactly the
# topics where catching it matters most. Negation is what actually decides
# whether two rules with the same signature agree or contradict.
_NEGATED_RE = re.compile(r"^(?:never|no|not|do not|don't|must not|should not|avoid)\b")


def _is_negated(text: str) -> bool:
    return bool(_NEGATED_RE.search(normalize_text(text)))


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


# ----- AD3: rule-vs-rule contradiction detection -----
#
# evaluate_rule's tail loop only compares a candidate against the frozen
# AUTHORITY SOURCES (AGENTS.md / CLAUDE.md / .claude/rules) — it cannot catch
# two MINED rules that contradict each other, since neither is a source.
# This reuses the exact same literal-signature + restrictive-mismatch test
# against a caller-supplied set of stored rule dicts. Deterministic/lexical
# only — no embeddings, no external model call.

def detect_rule_contradictions(
    rule: str,
    *,
    scope: str,
    stored_rules: Iterable[dict],
) -> list[dict]:
    """Lexically scan `stored_rules` for a contradiction against `rule`.

    Two rules contradict when they reduce to the same literal signature
    (same subject after stripping a leading modal like "always"/"never") but
    one is restrictive and the other is not — e.g. "Always squash commits"
    vs "Never squash commits". Retired/deprecated/superseded stored rules
    (via an optional `lifecycle_state` key) are skipped — a resolved dispute
    should not keep re-triggering. Scope must overlap in either direction.

    Returns a list of `{"id", "rule", "reason"}` conflict dicts for the
    caller to surface for user resolution; this function does not decide
    admission by itself.
    """
    candidate_signature = _literal_signature(rule)
    candidate_negated = _is_negated(rule)
    conflicts: list[dict] = []
    for stored in stored_rules:
        stored_state = str(stored.get("lifecycle_state", "active") or "active").lower()
        if stored_state in {"retired", "deprecated", "superseded"}:
            continue
        stored_rule_text = str(stored.get("rule", "") or "")
        if not stored_rule_text:
            continue
        stored_scope = str(stored.get("scope", "workspace") or "workspace")
        if not (_scope_applies(stored_scope, scope) or _scope_applies(scope, stored_scope)):
            continue
        if _literal_signature(stored_rule_text) != candidate_signature:
            continue
        if not candidate_signature:
            continue
        stored_negated = _is_negated(stored_rule_text)
        if stored_negated != candidate_negated:
            conflicts.append({
                "id": stored.get("id", ""),
                "rule": stored_rule_text,
                "reason": "restrictive-mismatch",
            })
    return conflicts


def evaluate_rule(
    rule: str,
    *,
    scope: str,
    declared_effect: str | None = None,
    authority_manifest: dict | None = None,
    authority_root: Path | None = None,
    origin: str | None = None,
    evidence_text: str = "",
) -> AuthorityResult:
    """Evaluate one candidate, quarantining deterministic safety failures."""
    origin_result = evaluate_origin(origin, evidence_text or rule)
    if not origin_result.admitted:
        return AuthorityResult(
            False, origin_result.reason, classify_authority_effect(rule)
        )
    computed_effect = classify_authority_effect(rule)
    effect = computed_effect
    if declared_effect == "permission_expanding":
        effect = "permission_expanding"
    elif declared_effect == "restrictive" and computed_effect == "neutral":
        effect = "restrictive"
    if declared_effect == "security_weakening":
        effect = "security_weakening"
    # Refusal is decided here; the REASON is refined below. A candidate that
    # contradicts a specific line in the operator's own authority files gets
    # "authority-conflict" and a pointer to that line, which is actionable --
    # strictly more useful than the generic category, and the categorical
    # refusal is unchanged either way because `admitted` stays False.
    categorical_refusal = None
    if effect == "security_weakening":
        categorical_refusal = "security-weakening"
    elif effect == "permission_expanding":
        categorical_refusal = "permission-expanding"

    if authority_manifest is None:
        if categorical_refusal:
            return AuthorityResult(False, categorical_refusal, effect)
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
    candidate_negated = _is_negated(rule)
    for line in _authority_lines(authority_manifest, authority_root, scope):
        if _literal_signature(line) != candidate_signature:
            continue
        authority_negated = _is_negated(line)
        if authority_negated != candidate_negated:
            # More specific than any categorical reason: it names the operator's
            # own contradicting line, so it wins even when the candidate is also
            # security-weakening or permission-expanding.
            return AuthorityResult(False, "authority-conflict", effect)
    if categorical_refusal:
        return AuthorityResult(False, categorical_refusal, effect)
    return AuthorityResult(True, "ok", effect)
