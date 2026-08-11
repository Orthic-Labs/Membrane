"""Phase 5.3 — Taste (TranscriptEventV1 substrate).

Mission (plan 5.3, line 101):
    corrections + locked decisions become durable scoped rules with
    exact-span provenance -> Crypt -> surfaced via membrane_context.

This module is the Phase 5.3 path. It replaces retired session mining by consuming shared
TranscriptEventV1 substrate at ``tools/lib/orthic_transcripts`` and
preserving the bounded surrounding context of every correction — the
plan-named defect that the previous path dropped.

Kept strengths (existing invariants, regressing any is a failure):

    - user-origin isolation: only ``event.kind == "user_message"`` or
      the explicit decision markers in DECISION_PATTERNS establish
      authority. Assistant narration, tool output, repository prose, and
      Insights findings can NEVER become a Taste rule (enforced in
      ``_assert_authoritative_provenance`` below).
    - synthetic / meta filters: events with ``flags.synthetic`` or
      ``flags.meta`` are rejected at intake.
    - health-domain exclusion: explicitly named health terms
      (``HEALTH_DOMAINS``) are filtered out before extraction.
    - lifecycle states: candidate records carry a ``lifecycle_state``
      field aligned with ``preference_record.normalize_lifecycle_state``.

Fixed defect (plan 5.3):
    The previous mining path dropped the surrounding context of a
    correction. A correction without its context is unusable as a rule.
    Every candidate now carries a bounded CONTEXT block (default
    ``MAX_CONTEXT_BLOCKS = 4`` events on each side) with the exact byte
    spans of the source transcript, so the rule has line-by-line
    provenance and the surrounding ask is auditable.

Transport gap (plan defect 24, discovered by the Phase 5.4 agent):
    ``adapt/observable_events.py:42`` emits taste candidates as
    ``{event_id, trace_id, source}`` with NO content, and the Rust
    service has no event-content lookup route — only metadata + digest.
    So the transport-side candidates cannot carry the text needed for
    admission. This module deliberately does NOT consume that
    pipeline; it consumes the TranscriptEventV1 layer directly, where
    text is preserved. The honest limitation is stated here, in
    the function-level docstring of ``extract_candidates``, and in
    TRANSPORT_GAP_NOTE (in-product, surfaced to the caller).

NEVER-authoritative — enforced in CODE, not comments:
    The provenance check ``_assert_authoritative_provenance`` raises
    before any candidate is admitted. The list of rejected origins
    is hard-coded; linters cannot drop it.

Output schema: ``TasteCandidateV1`` (frozen dataclass). Carries:
    - rule text + scope + category + retention tuple
    - exact byte-span of the source event (``byteStart``, ``byteEnd``)
    - bounded surrounding context (events with their own byte spans)
    - source sessionId / host / transcriptId / parserDigest
    - lifecycle_state, evidence_id, evidence_text
    - transport_note: the honest gap
    - rule_id: deterministic, derived from byte-span + rule text
"""

from __future__ import annotations

import dataclasses
import copy
import hashlib
import json
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable

# Local imports live BELOW the module docstring so the public surface is
# readable; path bootstrap must happen before importing admission.
sys.path.insert(0, str(Path(__file__).resolve().parent))

import admission  # noqa: E402
import authority  # noqa: E402
import preference_record  # noqa: E402
import transcript_sources  # noqa: E402

# ---------------------------------------------------------------------------
# Public schema + constants
# ---------------------------------------------------------------------------

SCHEMA_VERSION = "adapt.taste-candidate.v1"

# Bounded surrounding context (defect fix). Each candidate carries up to
# MAX_CONTEXT_BLOCKS events on each side of the source event. This is the
# minimum audit envelope: enough to know what the user asked and what the
# agent did, without dragging the whole transcript into a rule record.
MAX_CONTEXT_BLOCKS = 4
MAX_CONTEXT_CHARS = 4_000  # second bound: total context bytes are clamped.

# Health-domain exclusion vocabulary (legacy invariant). Any candidate whose
# rule text or surrounding context mentions a named health domain is refused
# upstream of ``admission.admit`` so the audit trail does not record it as
# a "rejected" preference (a different class of failure).
HEALTH_DOMAINS: frozenset[str] = frozenset({
    "medical",
    "diagnosis",
    "diagnostic",
    "therapeutic",
    "therapy",
    "medication",
    "prescription",
    "dosage",
    "clinical",
    "patient",
    "disease",
    "symptom",
})

# Tokens used to identify a USER correction in a user_message text. The
# legacy constants in ``authority`` (corrections, locked decisions) are
# kept narrow; we mirror them here so the detector is deterministic and
# testable without the LLM lane.
_CORRECTION_PATTERNS: tuple[re.Pattern[str], ...] = (
    # "No, that's wrong / not right / not how / not what / not where"
    re.compile(r"(?im)\bno,?\s+that'?s?\s+(?:not\s+)?(?:right|what|how|where|why)\b"),
    re.compile(r"(?im)\bno,?\s+that'?s?\s+wrong\b"),
    # Text starting with a correction cue (no leading "no,")
    re.compile(r"(?im)^\s*(?:wrong|incorrect|not\s+right|not\s+quite|that'?s?\s+wrong)\b"),
    re.compile(r"(?im)^\s*(?:correction|please\s+stop|rule)\s*:"),
    # "never X again" / "don't X again" / "stop X" / "why did you" / "why are you"
    re.compile(r"(?im)\bnever\s+(?:do|use|run|write|commit|skip)\b[^.?!]*\b(?:again|like\s+that)\b"),
    re.compile(r"(?im)\bdon'?t\s+(?:do|use|run|write|commit)\b[^.?!]*\b(?:again|like\s+that)\b"),
    re.compile(r"(?im)\bstop\s+(?:doing|using|writing|skipping|generating)\b"),
    re.compile(r"(?im)\bwhy\s+(?:did|are)\s+you\b"),
)

# Locked-decision patterns: user explicitly pins a constraint. These are
# the via-bona-fide user-authority inputs that the legacy path treated as
# ``decision_or_constraint`` and that taste MUST still mine into a
# locked-decision record. They fire anywhere in the text — a correction
# followed by a locked decision is still a locked decision.
_DECISION_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(r"(?im)^\s*(?:decision|locked|constraint|invariant|rules?)\s*:\s*"),
    re.compile(r"(?im)\b(?:always|never)\s+(?:[A-Za-z][\w-]*\s+){1,8}"),
    re.compile(r"(?im)\b(?:use|prefer|avoid|require)\b.{0,80}\b(temporarily\s+)?(?:going\s+forward|from\s+now\s+on|henceforth)\b"),
)

# Reuse the same imperative gating as admission so the rule text we emit
# passes the shape gate without re-failing the same call site. The
# detector does NOT call admission.admit itself (the caller does, with
# rule + evidence); it only produces a draft with a shape that will
# satisfy ``rule_shape_valid``.

# Events we refuse to mine from at intake. The detector MUST reject
# events with any of these flags set; doing otherwise lets a synthetic
# or model-side narration create a durable rule.
_REJECTED_FLAGS = frozenset({"synthetic", "meta", "privateReasoningOmitted", "redacted"})
SOURCE_FLAG_NAMES: tuple[str, ...] = (
    "synthetic", "meta", "privateReasoningOmitted", "redacted", "isError", "isSidechain",
)
_FAILURE_CLASSIFICATIONS = frozenset({"unresolved_failure", "failed_verification"})


# ---------------------------------------------------------------------------
# In-product statement of the permanent transport boundary (plan defect 24).
# ---------------------------------------------------------------------------

TRANSPORT_GAP_NOTE: str = (
    "Taste candidates carry the source event's exact byte spans and bounded "
    "surrounding context. Direct TranscriptEventV1 input "
    "(tools/lib/orthic_transcripts) is the canonical semantic source. "
    "ObservableEventV1 transport (adapt/observable_events.py) is permanently "
    "metadata-only for lineage and Insights; it never carries or resolves "
    "Taste text and cannot mint Taste candidates."
)


# ---------------------------------------------------------------------------
# Failure classes — surfaced as exceptions, not silent filters.
# ---------------------------------------------------------------------------

class TasteV2Error(RuntimeError):
    """Raised for any hard refusal of a Taste candidate.

    These are NEVER swallowed and NEVER a soft warning. The boundary
    between an "accepted" and a "rejected" candidate is recorded as
    an explicit boolean on the candidate so a downstream consumer
    can see the refusal reason without re-deriving it.
    """


# ---------------------------------------------------------------------------
# TasteCandidateV1 — the output shape Crypt will eventually store.
# ---------------------------------------------------------------------------

VALID_LIFECYCLE_STATES: frozenset[str] = frozenset({
    "candidate",   # freshly mined; not yet accepted by curation.
    "active",      # admitted to Crypt.
    "retired",     # manually withdrawn.
    "deprecated",  # superseded by a newer rule.
    "superseded",  # replaced by an explicit successor.
    "pending",     # queued for review.
    "rejected",    # admission refused; audit trail only.
})


@dataclasses.dataclass(frozen=True)
class TasteCandidateV1:
    """One mined rule with exact-span provenance and bounded context.

    Fields:
        ruleId:                deterministic id over (scope, byteStart, byteEnd,
                               rule_text). Two runs over the same byte span
                               and the same rule text give the same id; an edit
                               to the rule text gives a different id.
        rule:                  the rule text (imperative, gated by rule_shape_valid).
        scope:                 workspace / global / repo / file. Always set.
        category:              one of admission.ALLOWED_CATEGORIES (or
                               DEFAULT_FALLBACK_CATEGORY when the LLM
                               returned something outside the taxonomy).
        recordType:            standing_preference | locked_decision |
                               operational_playbook | episodic_fact.
        sourceEventId:         the eventId of the user_message that produced
                               the candidate.
        sourceByteStart:       byteStart of the source user_message row.
        sourceByteEnd:         byteEnd of the source user_message row.
        sourceRowIndex:        rowIndex of the source user_message row.
        sourceSequence:        sequence number of the source event.
        sourceHost:            host identity (claude_code / codex).
        sourceSessionId:       session id (exact, not substring).
        sourceTranscriptId:    transcriptId (frozen).
        sourceParserDigest:    the parserDigest binding (SHA-256 of the
                               parser implementation bytes).
        contextEvents:         bounded surrounding context events as a list
                               of dicts. Each dict carries eventId, kind,
                               byteStart, byteEnd, text (truncated to
                               MAX_CONTEXT_CHARS across the whole block).
        contextByteSpans:      list of (byteStart, byteEnd) tuples for the
                               full source-side context window. Same length
                               as contextEvents. Auditable exactly.
        evidenceId:            deterministic id over the source byte span.
        evidenceText:          the verbatim user text (compact-formatted,
                               redacted where the layer redacts).
        lifecycleState:        starting state. ``candidate`` if the detector
                               emitted it; ``active`` after admission.admit
                               returns ok; ``rejected`` otherwise.
        admissionReason:       the reason admission returned (or 'ok').
        transportNote:         the in-product gap note (TASTE TRANSPORT
                               TRANSPARENCY, plan 5.3 / defect 24).
        authorityEffect:       one of authority.AUTHORITY_EFFECTS.
        proposedAt:            ISO timestamp of the candidate's creation.
    """

    ruleId: str
    rule: str
    scope: str
    category: str
    recordType: str
    sourceEventId: str
    sourceByteStart: int
    sourceByteEnd: int
    sourceRowIndex: int
    sourceSequence: int
    sourceHost: str
    sourceSessionId: str
    sourceTranscriptId: str
    sourceParserDigest: str
    contextEvents: list[dict[str, Any]]
    contextByteSpans: list[tuple[int, int]]
    evidenceId: str
    evidenceText: str
    lifecycleState: str = "candidate"
    admissionReason: str = ""
    transportNote: str = TRANSPORT_GAP_NOTE
    authorityEffect: str = "neutral"
    proposedAt: str = ""
    sourceKind: str = ""
    sourceRole: str = ""
    sourceClassification: str = ""
    sourceFlags: tuple[tuple[str, bool], ...] = ()

    def __post_init__(self) -> None:
        flags = tuple(sorted((str(k), value) for k, value in self.sourceFlags))
        if tuple(name for name, _ in flags) != tuple(sorted(SOURCE_FLAG_NAMES)) or any(
            not isinstance(value, bool) for _, value in flags
        ):
            raise TasteV2Error("sourceFlags must contain exactly the six boolean parser flags")
        object.__setattr__(self, "sourceFlags", flags)
        if not all(str(value) for value in (
            self.ruleId, self.rule, self.sourceEventId, self.sourceSessionId,
            self.sourceTranscriptId, self.sourceParserDigest, self.sourceKind,
            self.sourceRole, self.sourceClassification, self.evidenceId, self.evidenceText,
        )):
            raise TasteV2Error("candidate immutable identity fields must be non-empty")
        if self.lifecycleState not in VALID_LIFECYCLE_STATES:
            raise TasteV2Error(
                f"lifecycleState must be one of {sorted(VALID_LIFECYCLE_STATES)!r}, "
                f"got {self.lifecycleState!r}"
            )
        if not self.ruleId:
            raise TasteV2Error("ruleId is required")
        if not self.rule:
            raise TasteV2Error("rule is required")
        if self.sourceByteStart < 0 or self.sourceByteEnd < self.sourceByteStart:
            raise TasteV2Error(
                f"source byte span is invalid: start={self.sourceByteStart} "
                f"end={self.sourceByteEnd}"
            )
        if not self.contextEvents:
            raise TasteV2Error(
                "contextEvents must be non-empty (defect fix: a correction "
                "without its context is unusable as a rule)"
            )
        if len(self.contextEvents) != len(self.contextByteSpans):
            raise TasteV2Error(
                "contextEvents and contextByteSpans must have the same length"
            )
        for event, span in zip(self.contextEvents, self.contextByteSpans):
            if not isinstance(event, dict) or not isinstance(span, tuple) or len(span) != 2:
                raise TasteV2Error("context events and spans must have exact object/pair shapes")
            start, end = span
            if not isinstance(start, int) or not isinstance(end, int) or start < 0 or end < start:
                raise TasteV2Error("context byte span is invalid")
            required_context_fields = {
                "eventId", "kind", "role", "classification", "flags", "byteStart", "byteEnd",
                "text", "truncated", "isSource", "provenance", "sourceRows", "authorityEligible",
            }
            if set(event) != required_context_fields:
                raise TasteV2Error("context event has an invalid envelope shape")
            if not all(isinstance(event[field], str) for field in ("eventId", "kind", "classification", "text")):
                raise TasteV2Error("context event identity/text fields must be strings")
            if event["role"] is not None and not isinstance(event["role"], str):
                raise TasteV2Error("context event role must be string or null")
            if not isinstance(event["truncated"], bool) or not isinstance(event["isSource"], bool):
                raise TasteV2Error("context event flags must be booleans")
            if event["provenance"] not in transcript_sources.PROVENANCE_KINDS:
                raise TasteV2Error("context event provenance is invalid")
            if not isinstance(event["sourceRows"], list) or not event["sourceRows"]:
                raise TasteV2Error("context event sourceRows must be non-empty")
            if not isinstance(event["authorityEligible"], bool):
                raise TasteV2Error("context event authorityEligible must be boolean")
            if event["byteStart"] != start or event["byteEnd"] != end:
                raise TasteV2Error("context event byte span does not match contextByteSpans")
            event_flags = event["flags"]
            if not isinstance(event_flags, dict) or set(event_flags) != set(SOURCE_FLAG_NAMES) or any(
                not isinstance(value, bool) for value in event_flags.values()
            ):
                raise TasteV2Error("context event flags must be exact six booleans")
        sources = [event for event in self.contextEvents if event.get("isSource") is True]
        if len(sources) != 1:
            raise TasteV2Error("contextEvents must contain exactly one source event")
        source = sources[0]
        required_context_fields = {
            "eventId", "kind", "role", "classification", "flags", "byteStart", "byteEnd",
            "text", "truncated", "isSource", "provenance", "sourceRows", "authorityEligible",
        }
        if set(source) != required_context_fields or source["truncated"] is not False:
            raise TasteV2Error("source context must preserve the exact untruncated envelope")
        expected_source = {
            "eventId": self.sourceEventId, "kind": self.sourceKind, "role": self.sourceRole,
            "classification": self.sourceClassification, "flags": dict(self.sourceFlags),
            "byteStart": self.sourceByteStart, "byteEnd": self.sourceByteEnd, "text": self.evidenceText,
        }
        if any(source[field] != value for field, value in expected_source.items()):
            raise TasteV2Error("source context must exactly match candidate provenance")


# ---------------------------------------------------------------------------
# Internal detectors
# ---------------------------------------------------------------------------

def _is_correction(text: str) -> bool:
    return any(p.search(text) for p in _CORRECTION_PATTERNS)


def _is_decision(text: str) -> bool:
    return any(p.search(text) for p in _DECISION_PATTERNS)


def _normalise_rule_from_correction(text: str) -> str:
    """Strip the leading correction cue and tidy the rule text.

    The detector does NOT emit a polished rule — admission.admit and
    PreferenceRecord do the heavy lifting. We only strip the leading
    negative / "no" / "wrong" / "stop" cue so the rule text is the
    imperative that the user actually meant.
    """
    cleaned = text.strip()
    # Drop leading correction cues (anywhere in the text is fine; leading
    # occurrences first so the rule starts with the imperative).
    cleaned = re.sub(
        r"(?im)^\s*(?:no,?\s+that'?s?\s+(?:not\s+)?(?:right|what|how|where|why)\s*[:,]?\s*"
        r"|no,?\s+that'?s?\s+wrong\s*[:,]?\s*"
        r"|wrong\s*[:,]?\s*"
        r"|incorrect\s*[:,]?\s*"
        r"|stop\s+(?:doing|using|writing|skipping|generating)\s*[:,]?\s*"
        r"|why\s+did\s+you\s*[:,]?\s*"
        r"|(?:correction|please\s+stop|rule)\s*:\s*)",
        "",
        cleaned,
    ).strip()
    # If a sentence-break appears (". Always ..."), keep the second clause.
    # This is what surfaces the *actual* rule from a correction like
    # "No, that's wrong. Always run focused tests first."
    parts = re.split(r"\.\s+", cleaned, maxsplit=1)
    if len(parts) == 2 and parts[1].strip():
        cleaned = parts[1].strip()
    if not cleaned:
        # Fall back to the verbatim text so admission can reject it
        # explicitly rather than us silently dropping it.
        return text.strip()
    return cleaned


def _infer_record_type(text: str) -> str:
    """Heuristic record-type choice from the surface form.

    Locked decisions -> locked_decision. Repeated or imperative standing
    cues -> operational_playbook. Everything else -> episodic_fact.
    """
    if _is_decision(text):
        return "locked_decision"
    if _is_correction(text):
        return "operational_playbook"
    return "episodic_fact"


def _infer_category(text: str) -> str:
    """Lexical best-effort category. Falls back to ``workflow``."""
    low = text.lower()
    if any(w in low for w in ("test", "pytest", "spec", "lint", "type-check", "verify")):
        return "verification"
    if any(w in low for w in ("safe", "fail closed", "fail-closed", "permission", "auth", "credential")):
        return "safety"
    if any(w in low for w in ("architecture", "module", "layer", "interface", "abstraction")):
        return "architecture"
    if any(w in low for w in ("style", "format", "naming", "quote", "indent")):
        return "code-style"
    if any(w in low for w in ("doc", "readme", "comment", "docstring")):
        return "documentation"
    if any(w in low for w in ("tool", "cli", "command", "pipeline", "script")):
        return "tooling"
    if any(w in low for w in ("fable", "opus", "sonnet", "minimax", "codex", "haiku", "model")):
        return "model-routing"
    return "workflow"


def _block_intended_health(text: str) -> bool:
    low = text.lower()
    return any(term in low for term in HEALTH_DOMAINS)


def _assert_authoritative_provenance(event: dict[str, Any]) -> None:
    """NEVER-authoritative enforcement (plan 5.3).

    A Taste candidate can only be minted from a user_message that:

      - has NO synthetic / meta / privateReasoningOmitted / redacted flag set
      - is NOT classified as ``unresolved_failure`` / ``failed_verification``
        (a *tool* failure is not a user preference)
      - has text that matches a correction or a decision cue

    Anything else — assistant narration, tool output, repository prose,
    Insights findings — is refused BEFORE we even build a candidate.
    """
    if not isinstance(event, dict):
        raise TasteV2Error("authoritative-provenance-rejected: source event must be an object")
    event_id = event.get("eventId")
    if not isinstance(event_id, str) or not event_id:
        raise TasteV2Error("authoritative-provenance-rejected: source eventId is required")
    flags = event.get("flags")
    if not isinstance(flags, dict) or set(flags) != set(SOURCE_FLAG_NAMES) or any(
        not isinstance(value, bool) for value in flags.values()
    ):
        raise TasteV2Error("authoritative-provenance-rejected: source flags must be exact six booleans")
    bad_flags = sorted(name for name, value in flags.items() if value)
    if bad_flags:
        raise TasteV2Error(
            f"authoritative-provenance-rejected: flag set is "
            f"{bad_flags!r} (synthetic/meta/redacted/privateReasoningOmitted "
            f"cannot establish rule authority)"
        )
    kind = str(event.get("kind") or "")
    if kind != "user_message":
        raise TasteV2Error(
            f"authoritative-provenance-rejected: kind={kind!r} is not "
            f"user_message; assistant narration, tool output, and repository "
            f"prose cannot establish rule authority"
        )
    if event.get("role") != "user":
        raise TasteV2Error("authoritative-provenance-rejected: role must be 'user'")
    if transcript_sources.event_provenance(event) != "external_user":
        raise TasteV2Error("authoritative-provenance-rejected: provenance must be external_user")
    classification = event.get("classification")
    if not isinstance(classification, str) or not classification:
        raise TasteV2Error("authoritative-provenance-rejected: source classification is required")
    if classification in _FAILURE_CLASSIFICATIONS:
        raise TasteV2Error(
            f"authoritative-provenance-rejected: classification={classification!r} "
            f"is a tool-failure signal, not a user preference"
        )

def _validated_candidate_source(candidate: TasteCandidateV1) -> dict[str, Any]:
    """Return the sole source envelope after exact candidate/context equality checks."""
    expected = {
        "eventId": candidate.sourceEventId,
        "kind": candidate.sourceKind,
        "role": candidate.sourceRole,
        "classification": candidate.sourceClassification,
        "flags": dict(candidate.sourceFlags),
        "byteStart": candidate.sourceByteStart,
        "byteEnd": candidate.sourceByteEnd,
        "text": candidate.evidenceText,
    }
    sources = [event for event in candidate.contextEvents if event.get("isSource") is True]
    if len(sources) != 1:
        raise TasteV2Error("candidate source context must contain exactly one source event")
    source = sources[0]
    if set(source) != {"eventId", "kind", "role", "classification", "flags", "byteStart", "byteEnd", "text", "truncated", "isSource", "provenance", "sourceRows", "authorityEligible"}:
        raise TasteV2Error("candidate source context has an invalid envelope shape")
    if source["truncated"] is not False:
        raise TasteV2Error("candidate source context must preserve untruncated evidence text")
    for field, value in expected.items():
        if source[field] != value:
            raise TasteV2Error(f"candidate source context mismatch: {field}")
    source_index = candidate.contextEvents.index(source)
    if tuple(candidate.contextByteSpans[source_index]) != (source["byteStart"], source["byteEnd"]):
        raise TasteV2Error("candidate source context mismatch: span")
    _assert_authoritative_provenance(source)
    return source


def _rejected_candidate(candidate: TasteCandidateV1, reason: str) -> TasteCandidateV1:
    """Stamp a malformed post-construction candidate without re-validating it."""
    rejected = copy.copy(candidate)
    object.__setattr__(rejected, "lifecycleState", "rejected")
    object.__setattr__(rejected, "admissionReason", reason)
    return rejected


def _bounded_context(
    events: list[dict[str, Any]],
    index: int,
    *,
    max_blocks: int = MAX_CONTEXT_BLOCKS,
    max_chars: int = MAX_CONTEXT_CHARS,
) -> tuple[list[dict[str, Any]], list[tuple[int, int]]]:
    """Return (events, spans) for the bounded surrounding context.

    Defect fix: a correction without its context is unusable as a rule.
    The default is 4 events on each side of the source. Context is also
    char-bound so a parser configuration that emits gigantic tool
    results cannot blow up the rule record.

    The list is anchored on the source event: the source event is at
    the centre, and we walk outward, accumulating up to ``max_blocks``
    events on each side. We always include the source event itself so
    the byte span lines up with ``sourceByteStart``/``sourceByteEnd``.
    """
    if not events:
        return [], []
    if not (0 <= index < len(events)):
        raise TasteV2Error(f"context index out of range: {index} / {len(events)}")

    source = events[index]
    left = events[max(0, index - max_blocks):index]
    right = events[index + 1:index + 1 + max_blocks]
    ordered = list(left) + [source] + list(right)

    total_chars = 0
    clipped: list[dict[str, Any]] = []
    spans: list[tuple[int, int]] = []
    for ev in ordered:
        text = str(ev.get("text") or "")
        start = int(ev.get("byteStart") or 0)
        end = int(ev.get("byteEnd") or 0)
        is_source = ev is source
        # Source event must always be present (so its byte span anchors
        # the record). For everything else, never overflow the cap.
        if total_chars >= max_chars and not is_source:
            continue
        remaining = max_chars - total_chars
        truncated = not is_source and len(text) > remaining
        context_text = text[:remaining] + "…" if truncated else text
        clipped.append({
            "eventId": ev.get("eventId", ""),
            "kind": ev.get("kind", ""),
            "role": ev.get("role"),
            "classification": ev.get("classification"),
            "flags": {name: bool((ev.get("flags") or {}).get(name)) for name in SOURCE_FLAG_NAMES},
            "byteStart": start,
            "byteEnd": end,
            "text": context_text,
            "truncated": truncated,
            "isSource": is_source,
            "provenance": transcript_sources.event_provenance(ev),
            "sourceRows": list(ev.get("sourceRows") or [{
                "eventId": str(ev.get("eventId") or ""), "rowIndex": int(ev.get("rowIndex") or 0),
                "byteStart": start, "byteEnd": end, "projection": str(ev.get("projection") or "default"),
            }]),
            "authorityEligible": transcript_sources.event_provenance(ev) == "external_user",
        })
        if not is_source:
            total_chars += min(len(text), remaining)
        spans.append((start, end))
    return clipped, spans


def _rule_id(scope: str, rule: str, byte_start: int, byte_end: int) -> str:
    seed = json.dumps(
        {
            "scope": scope,
            "byteStart": byte_start,
            "byteEnd": byte_end,
            "rule": rule,
        },
        sort_keys=True,
        ensure_ascii=False,
    )
    return "taste_" + hashlib.sha256(seed.encode("utf-8")).hexdigest()[:24]


def _evidence_id(scope: str, evidence: str) -> str:
    seed = json.dumps(
        {"scope": scope, "evidence": evidence},
        sort_keys=True,
        ensure_ascii=False,
    )
    return "ev_" + hashlib.sha256(seed.encode("utf-8")).hexdigest()[:20]


def _now_iso() -> str:
    import datetime as _dt
    return _dt.datetime.now(_dt.timezone.utc).isoformat()


def _build_candidate(
    events: list[dict[str, Any]],
    index: int,
    rule_text: str,
    *,
    scope: str,
    category: str | None = None,
    record_type: str | None = None,
    max_blocks: int = MAX_CONTEXT_BLOCKS,
    max_chars: int = MAX_CONTEXT_CHARS,
) -> TasteCandidateV1 | None:
    """Bind one proposed rule to one exact authoritative source event."""
    event = events[index]
    _assert_authoritative_provenance(event)
    context_events, context_spans = _bounded_context(
        events, index, max_blocks=max_blocks, max_chars=max_chars,
    )
    if not context_events:
        return None
    resolved_scope = scope or "workspace"
    resolved_category = admission.normalize_category(category or _infer_category(rule_text))
    byte_start = int(event.get("byteStart") or 0)
    byte_end = int(event.get("byteEnd") or 0)
    lifecycle_state = preference_record.normalize_lifecycle_state("candidate")
    if lifecycle_state not in VALID_LIFECYCLE_STATES:
        lifecycle_state = "candidate"
    return TasteCandidateV1(
        ruleId=_rule_id(resolved_scope, rule_text, byte_start, byte_end),
        rule=rule_text,
        scope=resolved_scope,
        category=resolved_category,
        recordType=record_type or _infer_record_type(rule_text),
        sourceEventId=str(event.get("eventId") or ""),
        sourceByteStart=byte_start,
        sourceByteEnd=byte_end,
        sourceRowIndex=int(event.get("rowIndex") or 0),
        sourceSequence=int(event.get("sequence") or 0),
        sourceHost=str(event.get("host") or ""),
        sourceSessionId=str(event.get("sessionId") or ""),
        sourceTranscriptId=str(event.get("transcriptId") or ""),
        sourceParserDigest=str(event.get("parserDigest") or ""),
        sourceKind=str(event.get("kind") or ""),
        sourceRole=str(event.get("role") or ""),
        sourceClassification=str(event.get("classification") or event.get("class") or ""),
        sourceFlags=tuple((name, bool((event.get("flags") or {}).get(name))) for name in SOURCE_FLAG_NAMES),
        contextEvents=context_events,
        contextByteSpans=context_spans,
        evidenceId=_evidence_id(resolved_scope, rule_text),
        evidenceText=str(event.get("text") or ""),
        lifecycleState=lifecycle_state,
        admissionReason="",
        authorityEffect=authority.classify_authority_effect(rule_text),
        proposedAt=_now_iso(),
    )


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def iter_candidate_indices(events: list[dict[str, Any]]) -> Iterable[int]:
    """Yield indices of events that are valid candidate sources.

    A valid candidate source is a user_message with a correction OR a
    locked decision cue, that has no synthetic / meta / redacted flag,
    and whose text is not in the health domain.
    """
    for index, event in enumerate(events):
        if not isinstance(event, dict):
            continue
        kind = str(event.get("kind") or "")
        if kind != "user_message":
            continue
        if transcript_sources.event_provenance(event) != "external_user":
            continue
        flags = event.get("flags") or {}
        if any(flags.get(name) for name in _REJECTED_FLAGS):
            continue
        text = str(event.get("text") or "")
        if not text:
            continue
        if _block_intended_health(text):
            continue
        if not (_is_correction(text) or _is_decision(text)):
            continue
        yield index


def iter_proposer_indices(events: list[dict[str, Any]]) -> Iterable[int]:
    """Yield safe external-user turns that deterministic extraction missed."""
    deterministic = set(iter_candidate_indices(events))
    for index, event in enumerate(events):
        if index in deterministic or not isinstance(event, dict):
            continue
        text = str(event.get("text") or "")
        if not text or _block_intended_health(text):
            continue
        try:
            _assert_authoritative_provenance(event)
        except TasteV2Error:
            continue
        yield index


def extract_candidate(
    events: list[dict[str, Any]],
    index: int,
    *,
    scope: str = "workspace",
    max_blocks: int = MAX_CONTEXT_BLOCKS,
    max_chars: int = MAX_CONTEXT_CHARS,
) -> TasteCandidateV1 | None:
    """Build one TasteCandidateV1 from the source event at ``index``.

    Returns ``None`` if the event is not a valid candidate source (the
    caller is expected to filter via ``iter_candidate_indices`` first).
    Raises ``TasteV2Error`` on hard refusals — distinct from "not a
    candidate", which is a soft ``None``.

    Consumed directly from the TranscriptEventV1 substrate because the
    transport-side content path is unavailable (see TRANSPORT_GAP_NOTE).
    """
    if not events:
        return None
    if not (0 <= index < len(events)):
        raise TasteV2Error(f"index out of range: {index} / {len(events)}")
    event = events[index]
    if not isinstance(event, dict):
        return None

    # Pre-flight: refuse any event that is not authoritative. This is
    # the NEVER-authoritative enforcement — code, not comments.
    if str(event.get("kind") or "") != "user_message":
        return None
    flags = event.get("flags") or {}
    if any(flags.get(name) for name in _REJECTED_FLAGS):
        return None
    text = str(event.get("text") or "")
    if not text:
        return None
    if _block_intended_health(text):
        return None
    if not (_is_correction(text) or _is_decision(text)):
        return None

    rule_text = _normalise_rule_from_correction(text)
    if not rule_text:
        return None
    return _build_candidate(
        events, index, rule_text, scope=scope,
        max_blocks=max_blocks, max_chars=max_chars,
    )


def propose_candidate(
    events: list[dict[str, Any]],
    index: int,
    rule: str,
    *,
    category: str = "",
    scope: str = "workspace",
    record_type: str = "standing_preference",
    max_blocks: int = MAX_CONTEXT_BLOCKS,
    max_chars: int = MAX_CONTEXT_CHARS,
) -> TasteCandidateV1 | None:
    """Bind an LLM proposal to one canonical external-user event.

    LLM output supplies rule wording only. Original event supplies authority,
    evidence, span, context, session identity, flags, & parser binding.
    """
    if not events or not (0 <= index < len(events)):
        return None
    event = events[index]
    text = str(event.get("text") or "") if isinstance(event, dict) else ""
    rule_text = " ".join(str(rule or "").split()).strip()
    if not text or not rule_text or _block_intended_health(text):
        return None
    return _build_candidate(
        events, index, rule_text, scope=scope, category=category, record_type=record_type,
        max_blocks=max_blocks, max_chars=max_chars,
    )


def extract_candidates(
    events: list[dict[str, Any]],
    *,
    scope: str = "workspace",
    scope_for_event=None,
    max_blocks: int = MAX_CONTEXT_BLOCKS,
    max_chars: int = MAX_CONTEXT_CHARS,
) -> list[TasteCandidateV1]:
    """Run the detector over a transcript event list and return every valid candidate.

    Consumes TranscriptEventV1 rows directly. The transport-side path
    (adapt/observable_events.py:42) cannot carry the text needed for
    admission — see TRANSPORT_GAP_NOTE.
    """
    canonical, _receipt = transcript_sources.canonicalize_events(events)
    canonical = [event for event in canonical if event.get("evidenceEligible") is not False]
    candidates: list[TasteCandidateV1] = []
    for index in iter_candidate_indices(canonical):
        event_scope = scope_for_event(canonical[index]) if scope_for_event else scope
        candidate = extract_candidate(
            canonical,
            index,
            scope=event_scope,
            max_blocks=max_blocks,
            max_chars=max_chars,
        )
        if candidate is not None:
            candidates.append(candidate)
    return candidates


# ---------------------------------------------------------------------------
# Admission gate — the canonical refuse / accept at the candidate boundary.
# ---------------------------------------------------------------------------

def admit_candidate(
    candidate: TasteCandidateV1,
    *,
    canonical_rules: dict[str, dict[str, Any]] | None = None,
    authority_manifest: dict | None = None,
    authority_root: "Path | None" = None,
    stored_rules: list[dict[str, Any]] | None = None,
) -> TasteCandidateV1:
    """Run the candidate through ``admission.admit`` and stamp the lifecycle.

    Returns a new TasteCandidateV1 with ``lifecycleState`` in
    ``active`` (admitted) or ``rejected`` (refused). The reason is
    stamped on the candidate so downstream consumers can see WHY
    without re-running the gate.

    The provenance check is RE-RUN here as defence-in-depth: a future
    caller that bypasses iter_candidate_indices and hand-rolls a
    candidate will still be refused here.
    """
    # Re-enforce provenance as defence in depth: hand-rolled candidates
    # could carry the wrong origin / classification, and we want the
    # rule to refuse them at the gate, not silently propagate.
    try:
        _validated_candidate_source(candidate)
    except TasteV2Error as exc:
        return _rejected_candidate(candidate, str(exc))

    target = {
        "name": candidate.ruleId,
        "rule": candidate.rule,
        "scope": candidate.scope,
        "category": candidate.category,
        "authority_effect": candidate.authorityEffect,
        "origin": "user_turn",
        "evidence_text": candidate.evidenceText,
        "action": "add",
    }
    try:
        admitted, why = admission.admit(
            "add",
            target,
            canonical_rules=canonical_rules,
            authority_manifest=authority_manifest,
            authority_root=authority_root,
            stored_rules=stored_rules,
        )
    except Exception as exc:  # noqa: BLE001
        return dataclasses.replace(
            candidate,
            lifecycleState="rejected",
            admissionReason=f"admission-error:{type(exc).__name__}:{exc}",
        )

    if admitted:
        return dataclasses.replace(
            candidate,
            lifecycleState="active",
            admissionReason="ok",
        )
    return dataclasses.replace(
        candidate,
        lifecycleState="rejected",
        admissionReason=str(why),
    )


# ---------------------------------------------------------------------------
# Public summary report (no writes; safe to call from CLI).
# ---------------------------------------------------------------------------

def summarise(candidates: list[TasteCandidateV1]) -> dict[str, Any]:
    """Group by lifecycle_state and emit a small serialisable summary.

    Intended for the operator: a single dict that shows how many
    rules were admitted, how many were refused, and why — without
    exposing the raw candidate payloads.
    """
    by_state: dict[str, int] = defaultdict(int)
    reasons: dict[str, list[str]] = defaultdict(list)
    for c in candidates:
        by_state[c.lifecycleState] += 1
        if c.lifecycleState == "rejected" and c.admissionReason:
            reasons[c.admissionReason].append(c.ruleId)
    return {
        "schema": SCHEMA_VERSION,
        "candidateCount": len(candidates),
        "byState": dict(by_state),
        "rejectedReasons": dict(reasons),
    }


# Convenience alias for the CLI printer.
summarize = summarise


# ---------------------------------------------------------------------------
# Phase 5.3 ends here. The internal module docstring state is the
# authoritative reference for the new boundary; the CLAUDE.md rules
# referenced at the top of this file are unchanged.
# ---------------------------------------------------------------------------

__all__ = [
    "SCHEMA_VERSION",
    "TasteCandidateV1",
    "TasteV2Error",
    "TRANSPORT_GAP_NOTE",
    "MAX_CONTEXT_BLOCKS",
    "MAX_CONTEXT_CHARS",
    "HEALTH_DOMAINS",
    "extract_candidate",
    "extract_candidates",
    "iter_candidate_indices",
    "admit_candidate",
    "summarise",
    "summarize",
]
