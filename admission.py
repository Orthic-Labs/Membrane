"""Admission policy + controlled taxonomy for morph.

A rule becomes a Crypt row only if it passes `admit()`. The synthetic
classifier is asked to output a category; anything outside the taxonomy is
remapped to `misc-review` so curation can act on it manually rather than have
the model invent duplicates (the audit-* triplet problem observed in the 81-row
backfill).
"""
from __future__ import annotations

import re
from pathlib import Path
from typing import Any

import authority
import preference_record
import rule_key

# Controlled taxonomy. Codex + Fable review 2026-07-12.
# Anything the model writes that's NOT in this set is forced to "misc-review".
ALLOWED_CATEGORIES: frozenset[str] = frozenset({
    "workflow",
    "verification",
    "safety",
    "architecture",
    "tooling",
    "code-style",
    "documentation",
    "model-routing",
})

DEFAULT_FALLBACK_CATEGORY = "misc-review"
_POLICY_DIR = Path(__file__).resolve().parent / "policies"
_MUTATION_ACTIONS = frozenset({"update", "deprecate"})
_ADD_ACTIONS = frozenset({"add"})
_ALLOWED_ACTIONS = _ADD_ACTIONS | _MUTATION_ACTIONS

_IMPERATIVE_STARTERS = frozenset({
    "always", "never", "use", "prefer", "run", "avoid", "stop", "do",
    "ensure", "require", "must", "keep", "check", "verify", "commit",
    "write", "read", "apply", "follow", "skip", "limit", "default",
})
_MIN_RULE_CHARS = 15


def normalize_category(raw: str) -> str:
    """Force the model's category into the controlled taxonomy.

    - Lowercased, trimmed.
    - Empty / unknown → DEFAULT_FALLBACK_CATEGORY ("misc-review").
    - Otherwise returned verbatim.
    """
    if not raw:
        return DEFAULT_FALLBACK_CATEGORY
    cat = raw.strip().lower()
    if cat in ALLOWED_CATEGORIES:
        return cat
    return DEFAULT_FALLBACK_CATEGORY


def _load_policy_profiles() -> list[dict[str, Any]]:
    profiles: list[dict[str, Any]] = []
    for path in sorted(_POLICY_DIR.glob("*.yaml")):
        text = path.read_text(encoding="utf-8")
        try:
            import yaml
            payload = yaml.safe_load(text)
        except ImportError:
            payload = _parse_simple_policy_yaml(text)
        if isinstance(payload, dict):
            profiles.append(payload)
    return profiles


def _parse_simple_policy_yaml(text: str) -> dict[str, Any]:
    """Minimal loader for the admission policy YAML shape when PyYAML is absent."""
    bans: list[dict[str, str]] = []
    current: dict[str, str] | None = None
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line == "regex_bans:":
            continue
        if line.startswith("- id:"):
            if current:
                bans.append(current)
            current = {"id": line.split(":", 1)[1].strip()}
            continue
        if current is not None and ":" in line:
            key, value = line.split(":", 1)
            current[key.strip()] = value.strip().strip("'\"")
    if current:
        bans.append(current)
    return {"regex_bans": bans}


def _compiled_policy_bans() -> list[tuple[str, re.Pattern[str]]]:
    bans: list[tuple[str, re.Pattern[str]]] = []
    for profile in _load_policy_profiles():
        for entry in profile.get("regex_bans") or ():
            if not isinstance(entry, dict):
                continue
            reason = str(entry.get("reason") or entry.get("id") or "policy-ban")
            pattern = entry.get("pattern")
            if not isinstance(pattern, str) or not pattern.strip():
                continue
            bans.append((reason, re.compile(pattern)))
    return bans


_POLICY_BANS = _compiled_policy_bans()


def rule_shape_valid(body: str) -> bool:
    """Require a durable sentence shape, without a brittle word-count gate."""
    normalized = preference_record.normalize_rule(body)
    if len(normalized) < _MIN_RULE_CHARS:
        return False
    words = normalized.split()
    first = words[0].rstrip("'t")
    if first in _IMPERATIVE_STARTERS:
        return True
    if f" {normalized} ".find(" should ") >= 0:
        return True
    if normalized.startswith("when "):
        return True
    return normalized.endswith((".", "!", "?"))


def _canonical_index(
    canonical_rules: set[str] | rule_key.RuleIndex | dict[str, dict[str, Any]] | None,
) -> rule_key.RuleIndex:
    if isinstance(canonical_rules, rule_key.RuleIndex):
        return canonical_rules
    if isinstance(canonical_rules, dict):
        return rule_key.RuleIndex.from_mapping(canonical_rules)
    index = rule_key.RuleIndex.from_mapping({})
    for value in canonical_rules or ():
        parsed = rule_key.RuleKey.parse(str(value))
        index.by_key.setdefault(parsed, {"id": parsed.record_id, "scope": parsed.scope})
    return index


def admit(
    action: str,
    target: dict,
    *,
    canonical_rules: set[str] | rule_key.RuleIndex | dict[str, dict[str, Any]] | None = None,
    authority_manifest: dict | None = None,
    authority_root: Path | None = None,
    stored_rules: list[dict] | None = None,
) -> tuple[bool, str]:
    """Decide whether to admit an action's rule to Crypt.

    Returns (admitted, reason). Reasons:
      "ok" — admitted.
      "category-not-allowed" — taxonomy refused it; review bucket.
      "rule-empty" — the rule text is empty after trim.
      "rule-duplicate" — an add collides with an existing scoped identity.
      "rule-invalid-shape" — too short or not a durable imperative/preference.
      "update-target-missing" / "deprecate-target-missing" — mutation with no target.
      "update-target-ambiguous" / "deprecate-target-ambiguous" — bare id matches many scopes.
      "permission-expanding" — inferred authority broadening is quarantined.
      "origin-not-user:*" (AD9) — non-user-origin evidence.
      "rule-conflict-needs-review" (AD3) — lexical contradiction with active rule.
      authority-manifest reasons — deterministic conflict/scope quarantine.
      policy-ban reasons — versioned regex bans from policies/*.yaml.

    Single-source admission policy. Tighter than the prompt's accuracy/precision,
    but is the only path to refuse the audit-* triplet pollution we observed.
    """
    operation = (action or target.get("action") or "add").strip().lower()
    if operation not in _ALLOWED_ACTIONS:
        return False, f"unsupported-action:{operation or 'missing'}"

    index = _canonical_index(canonical_rules)
    name = str(target.get("name") or target.get("id") or "").strip()
    scope = str(target.get("scope") or "").strip()
    body = (target.get("rule") or "").strip()
    cat = normalize_category(target.get("category", ""))

    if cat == DEFAULT_FALLBACK_CATEGORY:
        return False, "category-not-allowed"
    if not body:
        return False, "rule-empty"

    candidate_key = rule_key.RuleKey.for_target(name=name, scope=scope or None)
    resolved_key, _existing = index.resolve(name, scope=scope or None)
    if operation in _MUTATION_ACTIONS:
        if not name:
            return False, f"{operation}-target-missing"
        id_matches = index.keys_for_id(candidate_key.record_id)
        if resolved_key is None:
            if len(id_matches) > 1 and not scope:
                return False, f"{operation}-target-ambiguous"
            return False, f"{operation}-target-missing"
    elif operation == "add":
        if not candidate_key.record_id:
            return False, "add-target-missing"
        if index.has(candidate_key):
            return False, "rule-duplicate"
        if resolved_key is not None:
            return False, "rule-duplicate"
        if not scope and index.keys_for_id(candidate_key.record_id):
            return False, "rule-duplicate"

    for reason, pattern in _POLICY_BANS:
        if pattern.search(body):
            return False, reason

    authority_result = authority.evaluate_rule(
        body,
        scope=scope or candidate_key.scope,
        declared_effect=target.get("authority_effect"),
        authority_manifest=authority_manifest,
        authority_root=authority_root,
        origin=target.get("origin"),
        evidence_text=str(target.get("evidence_text") or target.get("evidence") or ""),
    )
    if not authority_result.admitted:
        return False, authority_result.reason
    if not rule_shape_valid(body):
        return False, "rule-invalid-shape"
    if stored_rules:
        conflicts = authority.detect_rule_contradictions(
            body,
            scope=scope or candidate_key.scope,
            stored_rules=stored_rules,
        )
        if conflicts:
            return False, "rule-conflict-needs-review"
    return True, "ok"
