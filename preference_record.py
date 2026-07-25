"""PreferenceRecord v1 — canonical identity + envelope for an admitted agent preference.

Implements the load-bearing contract from the v2 plan's Gate 2:

  - Adapt (NOT the model) assigns a stable ID at admission time.
  - ID = ``adapt-{category}-{slug}-{sha256(scope + NUL + category + NUL + normalized_rule)[:10]}``
  - Same input → same ID; distinct rule → distinct ID.
  - Updates keep the existing primary ID (in-place), matching Dream's rule.
  - Serialized into the existing MemRight content envelope; no schema column added.

Why a cryptographic suffix: slug collisions are inevitable (multiple "always use
JSONL"-style observations compress to the same kebab), and the model frequently
re-words an existing rule with a near-identical header. The 10-hex-char SHA
suffix makes accidental collisions astronomically unlikely and keeps the
deterministic ID property that downstream Dream/curation relies on.

The record is a frozen dataclass so the contract surface is explicit. The
frozen-ness also means a `replace()` is required to mutate, which forces every
update path to be deliberate.

Exposed helpers:
  - ``derive_id(scope, category, rule)`` — primary ID derivation.
  - ``PreferenceRecord.from_synthesis(action, *, scope, source_ids, existing=None)``
    — wraps a synthesis action into a PreferenceRecord, preserving the prior
    primary ID when ``existing`` is provided (update path).
  - ``to_memright_content(record)`` — formats into the existing
    ``**[adapt/{cat}]** — {rule} ...`` envelope without touching the engine
    schema.
"""
from __future__ import annotations

import dataclasses
import datetime as dt
import hashlib
import json
import os
import platform
import re
from pathlib import Path

import authority

# ----- Gate 2 contract surface -----

SCHEMA_VERSION = "1.2.0"
KIND = "preference"
PREFIX = "adapt"
HASH_LEN = 10
NUL = "\x00"
MAX_ALIAS_CHARS = 320
MAX_ALIASES = 3

# ----- Machine identity (attribution only — NOT part of the `scope` contract) -----
#
# `scope` (above) stays a workspace-recall partition and is IMMUTABLE_FIELDS in
# manifest.py / part of the payload_sha256 hash — never repurposed here.
# `machine` is a separate, optional attribution field: which installation
# recorded this rule. Default behavior (workspace-wide recall) is unaffected;
# `machine` is informational unless a caller explicitly narrows with
# `machine_only=True`.
_WORKSPACE_ROOT = Path(__file__).resolve().parents[4]


def _installation_identity_file() -> Path:
    """Same path/override contract as ``adapt._installation_file()``.

    Reuses the existing cross-machine installation identity
    (``tools/.cache/memory/installation.json``, schema v2, already loaded by
    ``cross_machine.load_installation_id``) instead of inventing a second
    identity file.
    """
    override = os.environ.get("ADAPT_INSTALLATION_FILE", "").strip()
    if override:
        return Path(override)
    return _WORKSPACE_ROOT / "tools/.cache/memory/installation.json"


def default_machine_id(*, installation_file: Path | None = None) -> str:
    """Best-effort, stable machine label for attributing a mined preference.

    Resolution order:
      1. ``ADAPT_MACHINE_ID`` env override (explicit control / tests).
      2. The existing installation identity file's ``legacy_labels[0]``
         (human-readable, e.g. ``"adrian-mac"``) — reused from the
         cross-machine Adapt pipeline rather than a new parallel identifier.
      3. That same file's ``installation_id`` (stable UUID) if no label.
      4. ``{platform.system()}-{platform.node()}`` (hostname) as a last
         resort when no installation identity has been set up yet.

    Never raises: a missing/corrupt identity file just falls through to the
    next step, since machine attribution is best-effort metadata, not a
    safety-gated contract field.
    """
    override = os.environ.get("ADAPT_MACHINE_ID", "").strip()
    if override:
        return override
    path = installation_file if installation_file is not None else _installation_identity_file()
    try:
        payload = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError):
        payload = None
    if isinstance(payload, dict):
        labels = payload.get("legacy_labels")
        if isinstance(labels, list) and labels and isinstance(labels[0], str) and labels[0].strip():
            return labels[0].strip()
        installation_id = payload.get("installation_id")
        if isinstance(installation_id, str) and installation_id.strip():
            return installation_id.strip()
    system = (platform.system() or "").strip()
    node = (platform.node() or "").strip()
    combined = "-".join(part for part in (system, node) if part)
    return combined or "unknown-machine"


# Frozen-set: the v2 plan treats this list as the contract fields.
REQUIRED_FIELDS: tuple[str, ...] = (
    "schema_version",
    "id",
    "kind",
    "rule",
    "category",
    "scope",
    "confidence",
    "needs_review",
    "evidence_count",
    "source_ids",
    "created_at",
    "updated_at",
    "record_type",
    "authority_effect",
    "status",
    "retrieval_aliases",
)

# Status values used inside the record itself. The manifest surface uses the
# same vocabulary; reviewer-side adds ``pending`` which is never allowed in
# apply-time inputs.
ALLOWED_STATUS: frozenset[str] = frozenset({"accepted", "rejected"})


# ----- ID derivation -----

_SLUG_RE = re.compile(r"[^a-z0-9]+")
_WORD_RE = re.compile(r"[a-zA-Z][a-zA-Z0-9]+")
_WS_RE = re.compile(r"\s+")
_TRAILING_PUNCT_RE = re.compile(r"[.!?,;:]+$")


def normalize_rule(rule: str) -> str:
    """Lowercase, collapse whitespace, strip trailing punctuation.

    Two rules that differ only in whitespace/case/punctuation hash to the
    same ID, which is the desired behavior for ``derive_id``. The original
    wording is still preserved on the record itself.
    """
    s = (rule or "").strip().lower()
    s = _WS_RE.sub(" ", s)
    s = _TRAILING_PUNCT_RE.sub("", s)
    return s


def normalize_retrieval_aliases(values, *, rule: str = "",
                                max_chars: int = MAX_ALIAS_CHARS,
                                max_aliases: int = MAX_ALIASES) -> tuple[str, ...]:
    """Deduplicate and bound sanitized source-language retrieval cues."""
    aliases: list[str] = []
    seen: set[str] = set()
    rule_norm = normalize_rule(rule)
    used = 0
    for value in values or ():
        text = _WS_RE.sub(" ", str(value or "")).strip()
        norm = normalize_rule(text)
        if not text or not norm or norm == rule_norm or norm in seen:
            continue
        remaining = max_chars - used
        if remaining <= 0 or len(aliases) >= max_aliases:
            break
        text = text[:remaining].rstrip()
        if not text:
            break
        aliases.append(text)
        seen.add(norm)
        used += len(text)
    return tuple(aliases)


def slug_from_rule(rule: str, max_words: int = 4) -> str:
    """3-5 kebab-case words from the rule body; falls back to 'preference'."""
    words = _WORD_RE.findall(rule or "")
    keep = [w.lower() for w in words[:max_words]]
    if not keep:
        keep = ["preference"]
    slug = _SLUG_RE.sub("-", "-".join(keep)).strip("-")
    return slug or "preference"


def derive_id(scope: str, category: str, rule: str) -> str:
    """``adapt-{category}-{slug}-{sha256(scope + NUL + category + NUL + normalized_rule)[:10]}``.

    ``NUL`` separator prevents the classic ambiguity where
    ``("ab", "cd")`` and ``("a", "bcd")`` would otherwise hash identically.

    For UPDATES, callers must pass the *initial* (pre-edit) rule to
    preserve identity. ``PreferenceRecord.from_synthesis(..., existing=...)``
    handles this: it takes the prior record's id verbatim.
    """
    norm = normalize_rule(rule)
    h = hashlib.sha256()
    h.update((scope or "").encode("utf-8"))
    h.update(NUL.encode("utf-8"))
    h.update((category or "").encode("utf-8"))
    h.update(NUL.encode("utf-8"))
    h.update(norm.encode("utf-8"))
    suffix = h.hexdigest()[:HASH_LEN]
    return f"{PREFIX}-{category}-{slug_from_rule(rule)}-{suffix}"


def _now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


# ----- Record -----

@dataclasses.dataclass(frozen=True)
class PreferenceRecord:
    schema_version: str
    id: str
    kind: str
    rule: str
    category: str
    scope: str
    confidence: float
    needs_review: bool
    evidence_count: int
    source_ids: tuple[str, ...]
    created_at: str
    updated_at: str
    record_type: str = "unclassified"
    authority_effect: str = "neutral"
    status: str = "accepted"
    retrieval_aliases: tuple[str, ...] = ()
    # Optional attribution: which installation recorded this rule. Absent on
    # every pre-existing record (backward compatible — NOT in REQUIRED_FIELDS,
    # NOT in manifest.IMMUTABLE_FIELDS, NOT part of any content hash). Empty
    # string means "unknown/not recorded", not "no machine".
    machine: str = ""
    # Optional narrowing: True means this rule applies only on `machine`.
    # Default False (the historical, unqualified behavior) means the rule
    # applies workspace-wide regardless of which machine recorded it.
    machine_only: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "source_ids", tuple(self.source_ids))
        object.__setattr__(self, "retrieval_aliases", normalize_retrieval_aliases(
            self.retrieval_aliases, rule=self.rule
        ))
        object.__setattr__(self, "machine", (self.machine or "").strip())
        object.__setattr__(self, "machine_only", bool(self.machine_only))

    def to_dict(self) -> dict:
        return {
            "schema_version": self.schema_version,
            "id": self.id,
            "kind": self.kind,
            "rule": self.rule,
            "category": self.category,
            "scope": self.scope,
            "confidence": self.confidence,
            "needs_review": self.needs_review,
            "evidence_count": self.evidence_count,
            "source_ids": list(self.source_ids),
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "record_type": self.record_type,
            "authority_effect": self.authority_effect,
            "status": self.status,
            "retrieval_aliases": list(self.retrieval_aliases),
            "machine": self.machine,
            "machine_only": self.machine_only,
        }

    @classmethod
    def from_synthesis(
        cls,
        action: dict,
        *,
        scope: str,
        source_ids: tuple[str, ...] | list[str],
        now: str | None = None,
        existing: dict | None = None,
        machine: str | None = None,
        machine_only: bool | None = None,
    ) -> "PreferenceRecord":
        """Wrap a synthesis action into a PreferenceRecord.

        `existing` (as a dict, e.g. from `rules.json`) preserves the prior
        primary id so updates keep identity.

        `machine`/`machine_only` are attribution metadata assigned by the
        caller (Adapt), never sourced from `action` (the LLM/operator
        payload) — mirrors the "Adapt, not the model, assigns identity"
        contract already used for `id`. Omitting `machine` (None) preserves
        whatever `existing` already recorded, defaulting to "" (unknown) for
        a brand-new record. Omitting `machine_only` (None) preserves
        `existing`'s narrowing, defaulting to False (applies everywhere) —
        so a caller that never mentions machine gets today's unqualified
        workspace-wide behavior, unchanged.
        """
        cat = action["category"]
        rule = (action.get("rule") or "").strip()
        if not rule:
            raise ValueError("synthesis action has empty rule")
        conf = float(action.get("confidence", 0.6))
        if existing:
            pid = existing.get("id") or existing.get("name")
            if not isinstance(pid, str) or not pid.strip():
                raise ValueError("existing preference has no stable id or name")
            created_at = existing.get("created_at", now or _now_iso())
        else:
            pid = derive_id(scope, cat, rule)
            created_at = now or _now_iso()
        record_type = authority.normalize_record_type(
            action.get("record_type", (existing or {}).get("record_type"))
        )
        declared_effect = action.get(
            "authority_effect", (existing or {}).get("authority_effect")
        )
        authority_effect = authority.evaluate_rule(
            rule,
            scope=scope,
            declared_effect=declared_effect,
        ).authority_effect
        resolved_machine = (
            machine if machine is not None else (existing or {}).get("machine", "")
        ) or ""
        resolved_machine_only = bool(
            machine_only if machine_only is not None
            else (existing or {}).get("machine_only", False)
        )
        return cls(
            schema_version=SCHEMA_VERSION,
            id=pid,
            kind=KIND,
            rule=rule,
            category=cat,
            scope=scope,
            confidence=conf,
            needs_review=bool(action.get("needs_review", conf < 0.5)),
            evidence_count=int(action.get("observations",
                                          action.get("evidence_count", 1))),
            source_ids=tuple(source_ids),
            created_at=created_at,
            updated_at=now or _now_iso(),
            record_type=record_type,
            authority_effect=authority_effect,
            status=action.get("status", "accepted"),
            retrieval_aliases=normalize_retrieval_aliases(
                action.get("retrieval_aliases", (existing or {}).get("retrieval_aliases", ())),
                rule=rule,
            ),
            machine=resolved_machine,
            machine_only=resolved_machine_only,
        )


def from_manifest_candidate(
    record: dict, *, now: str | None = None,
    machine: str | None = None, machine_only: bool | None = None,
) -> PreferenceRecord:
    """Convert an accepted manifest candidate into its canonical stored record.

    The manifest file itself never carries `machine`/`machine_only` (kept
    out of the schema-validated, hash-immutable candidate payload on
    purpose — see manifest.IMMUTABLE_FIELDS). Callers applying an accepted
    manifest may pass `machine` explicitly to attribute the record to the
    installation performing the apply; omitted, it stays "" (unknown),
    matching a legacy manifest record with no machine data at all.
    """
    return PreferenceRecord.from_synthesis(
        {
            "action": "add",
            "name": record["id"],
            "category": record["category"],
            "rule": record["rule"],
            "confidence": record.get("confidence", 0.6),
            "record_type": record.get("record_type", "unclassified"),
            "authority_effect": record.get("authority_effect", "neutral"),
            "retrieval_aliases": normalize_retrieval_aliases(
                record.get("retrieval_aliases", ()), rule=record.get("rule", "")
            ),
        },
        scope=record["scope"],
        source_ids=tuple(record.get("source_ids", [])),
        now=now,
        existing={
            "id": record["id"],
            "created_at": record.get("created_at", now or _now_iso()),
            "record_type": record.get("record_type", "unclassified"),
            "authority_effect": record.get("authority_effect", "neutral"),
            "retrieval_aliases": record.get("retrieval_aliases", ()),
        },
        machine=machine,
        machine_only=machine_only,
    )


# ----- Envelope (no MemRight schema change) -----

def application_guidance(record_type: str) -> str:
    """Return delivery-safe guidance for a typed record."""
    if record_type == "standing_preference":
        return "treat as a standing preference whenever the matching work comes up."
    if record_type == "locked_decision":
        return "apply as the current locked decision only inside its declared scope."
    if record_type == "operational_playbook":
        return "apply as a procedure only when the record scope matches the task."
    return "use as supporting context only; this is not a standing instruction."

def to_memright_content(record: PreferenceRecord) -> str:
    """Format into the existing ``**[adapt/{cat}]** — ...`` body.

    The engine-facing body is what MemRight stores. The category prefix and
    confidence line mirror what `adapt.rule_body` already produced so prior
    rows read identically.
    """
    today = dt.date.today().isoformat()
    aliases = (
        f"**Trigger phrases:** {' | '.join(record.retrieval_aliases)}\n"
        if record.retrieval_aliases else ""
    )
    machine_line = (
        f"**Machine:** {record.machine}"
        f"{' (machine-only)' if record.machine_only else ''}\n"
        if record.machine else ""
    )
    return (
        f"**[adapt/{record.category}]** — {record.rule} "
        f"Confidence: {record.confidence:.2f} "
        f"(observations: {record.evidence_count}, needs_review: "
        f"{str(record.needs_review).lower()}, updated {today})\n"
        f"**Why:** preference record; id={record.id}, evidence_count={record.evidence_count}\n"
        f"**Record:** type={record.record_type}, authority_effect={record.authority_effect}\n"
        f"{aliases}"
        f"{machine_line}"
        f"**How to apply:** {application_guidance(record.record_type)}\n"
    )


def to_manifest_candidate(record: PreferenceRecord,
                          *, evidence_excerpt: str = "",
                          human_note: str = "",
                          status: str = "pending",
                          payload_sha256: str = "") -> dict:
    """Shape the record as a candidate for the manifest emission.

    ``payload_sha256`` should be computed over the immutable candidate
    payload (``manifest.candidate_payload``); the manifest module owns the
    canonical hash, so callers pass it in.
    """
    return {
        "id": record.id,
        "rule": record.rule,
        "category": record.category,
        "scope": record.scope,
        "record_type": record.record_type,
        "authority_effect": record.authority_effect,
        "status": status,
        "confidence": record.confidence,
        "needs_review": record.needs_review,
        "evidence_count": record.evidence_count,
        "evidence_excerpt": evidence_excerpt,
        "source_ids": sorted(record.source_ids),
        "retrieval_aliases": list(record.retrieval_aliases),
        "human_note": human_note,
        "payload_sha256": payload_sha256,
    }
