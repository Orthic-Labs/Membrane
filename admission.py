"""Admission policy + controlled taxonomy for adapt.

A rule becomes a MemRight row only if it passes `admit()`. The synthetic
classifier is asked to output a category; anything outside the taxonomy is
remapped to `misc-review` so curation can act on it manually rather than have
the model invent duplicates (the audit-* triplet problem observed in the 81-row
backfill).
"""
from __future__ import annotations

from pathlib import Path

import authority

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


def admit(rule: dict, *, canonical_rules: set[str] | None = None,
          authority_manifest: dict | None = None,
          authority_root: Path | None = None) -> tuple[bool, str]:
    """Decide whether to admit an action's rule to MemRight.

    Returns (admitted, reason). Reasons:
      "ok" — admitted.
      "category-not-allowed" — taxonomy refused it; review bucket.
      "rule-empty" — the rule text is empty after trim.
      "rule-duplicate" — a canonical rule with the same name already exists.
      "rule-too-short" — fewer than 8 words (not durable).
      "permission-expanding" — inferred authority broadening is quarantined.
      authority-manifest reasons — deterministic conflict/scope quarantine.

    Single-source admission policy. Tighter than the prompt's accuracy/precision,
    but is the only path to refuse the audit-* triplet pollution we observed.
    """
    canonical_rules = canonical_rules or set()
    name = rule.get("name", "")
    body = (rule.get("rule") or "").strip()
    cat = normalize_category(rule.get("category", ""))

    if cat == DEFAULT_FALLBACK_CATEGORY:
        return False, "category-not-allowed"
    if not body:
        return False, "rule-empty"
    if name in canonical_rules:
        return False, "rule-duplicate"
    words = body.split()
    if len(words) < 8:
        return False, "rule-too-short"
    authority_result = authority.evaluate_rule(
        body,
        scope=str(rule.get("scope", "workspace")),
        declared_effect=rule.get("authority_effect"),
        authority_manifest=authority_manifest,
        authority_root=authority_root,
    )
    if not authority_result.admitted:
        return False, authority_result.reason
    return True, "ok"
