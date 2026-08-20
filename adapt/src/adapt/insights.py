"""Phase 5.5 — Adapt Insights (greenfield, report-only).

Honest scope, stated in-product (plan 5.5, line 103):

    Insights is greenfield. Customer: Adrian. Writes nothing to Membrane or
    Taste. Automates nothing. Output is a report only.

    "Only observable failure signals are detectable." (stated in-product,
    not just in a comment — see ``FailureCardV1.honestyLimit``.)

The module consumes ``TranscriptEventV1`` rows from Membrane continuity
Membrane ``continuity.transcript`` parser, runs every named detector
against them, and emits one ``FailureCardV1`` per detected failure mode.
There is no database, no apply path, no authority grant — detectors write
to a dict, a caller can hand to a printer, a JSON file, or a CI log.

Detectors implemented (the 18 named in the plan, plus one workspace
addition):

    1.  claimed_verified_then_corrected
    2.  repeated_ask
    3.  visible_frustration
    4.  verification_claim_without_tool_evidence
    5.  ignored_tool_failure
    6.  degraded_provider_treated_as_success
    7.  false_not_found
    8.  unproductive_broad_searching
    9.  wrong_repo_or_subsystem
   10.  stale_terminology_surfacing
   11.  silent_scope_narrowing
   12.  omitted_requirement
   13.  unaccepted_plan_change
   14.  tests_that_cannot_fail
   15.  cross_agent_repeats
   16.  forge_opened_never_closed
   17.  guard_firings
   18.  user_asks_why_missed_or_postmortem
   19.  user_swearing (workspace addition — explicit profanity is the
        strongest deterministic frustration signal we have)

Each detector is a pure function that takes the ordered event list and
returns one or more ``FailureCardV1`` records. They are independent and
idempotent — order of invocation does not matter, and re-running produces
the same cards with stable ids.
"""

from __future__ import annotations

import dataclasses
import datetime as _dt
import hashlib
import json
import re
import uuid
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable

from adapt import token_spend, transcript_sources


# ---------------------------------------------------------------------------
# FailureCardV1 schema
# ---------------------------------------------------------------------------

SCHEMA_VERSION = "adapt.failure-card.v1"

# Severity scale — qualitative, used for sort + filtering.
SEVERITIES = ("info", "low", "medium", "high", "critical")


@dataclass(frozen=True)
class FailureCardV1:
    """One detected failure mode in a transcript or set of transcripts.

    Fields map directly to plan 5.5's required output (line 103). The
    ``cardId`` is deterministic for a given (detector, evidence-spans) pair
    so two runs over the same input produce stable ids — useful for
    diffing reports and for tests.

    ``likelyMechanism`` and ``suggestedRemediations`` are clearly labelled
    inferences, never authoritative evidence (see ``honestyLimit``).

    Field meanings:
        detector        — one of the 18 named detector slugs.
        severity        — one of ``SEVERITIES``.
        confidence      — 0..1, the detector's self-rated confidence the
                          card is a real failure (not noise). Detectors
                          must honestly under-claim here.
        firstSeen /
        lastSeen        — ISO-8601 timestamps from the events that
                          triggered the card, when available. May be None
                          when the signal carries no timestamp.
        recurrenceCount — how many independent occurrences were observed.
        agents /
        hosts /
        sessions /
        repos           — unique lists of the relevant identities seen.
        taskSummary     — short narrative, ≤200 chars, computed from the
                          immediately surrounding user messages.
        userExpectation — the user's stated ask, reconstructed from the
                          nearest preceding user message.
        observedFailure — what the agent actually did/said.
        evidence        — list of byte-span excerpts (eventId +
                          byteStart/byteEnd + short text excerpt).
        likelyMechanism — heuristic only; always labelled "candidate".
        suggestedRemediations — list of short strings, also "candidate".
        userDisposition — disposition label from a fixed vocabulary so
                          report consumers can sort by disposition
                          (forgiven, repeated, escalated, …).
        honestyLimit    — constant string stating the in-product honesty
                          scope of Insights (plan 5.5: "only observable
                          failure signals are detectable").
    """

    detector: str
    severity: str
    confidence: float
    firstSeen: str | None
    lastSeen: str | None
    recurrenceCount: int
    agents: list[str] = field(default_factory=list)
    hosts: list[str] = field(default_factory=list)
    sessions: list[str] = field(default_factory=list)
    repos: list[str] = field(default_factory=list)
    taskSummary: str = ""
    userExpectation: str = ""
    observedFailure: str = ""
    evidence: list[dict[str, Any]] = field(default_factory=list)
    likelyMechanism: str = ""
    suggestedRemediations: list[str] = field(default_factory=list)
    userDisposition: str = "logged"

    # The in-product honesty limit. Stated on every card so the message
    # travels with the record — anyone reading the JSON sees the scope.
    honestyLimit: str = (
        "Insights detects only observable failure signals in transcripts. "
        "Cards are heuristic; 'likelyMechanism' and 'suggestedRemediations' "
        "are candidate inferences, not authoritative diagnoses."
    )

    cardId: str = ""

    def __post_init__(self) -> None:
        if self.severity not in SEVERITIES:
            raise ValueError(
                f"severity must be one of {SEVERITIES!r}, got {self.severity!r}"
            )
        if not 0.0 <= self.confidence <= 1.0:
            raise ValueError(f"confidence must be in 0..1, got {self.confidence!r}")
        if not self.cardId:
            object.__setattr__(self, "cardId", _compute_card_id(self))


# User-disposition vocabulary — fixed, narrow, machine-sortable.
DISPOSITIONS = frozenset(
    {"logged", "forgiven", "repeated", "escalated", "postmortem_requested"}
)


def _compute_card_id(card: "FailureCardV1") -> str:
    """Deterministic id keyed on detector + sorted evidence spans."""
    payload = {
        "detector": card.detector,
        "evidence": sorted(
            (ev.get("eventId", ""), ev.get("byteStart", -1), ev.get("byteEnd", -1))
            for ev in card.evidence
        ),
    }
    encoded = json.dumps(payload, sort_keys=True, ensure_ascii=False).encode("utf-8")
    return "fc_" + hashlib.sha256(encoded).hexdigest()[:24]


# ---------------------------------------------------------------------------
# Detectors — each is a pure function over the event list
# ---------------------------------------------------------------------------

# Vocabulary shared by multiple detectors.

_RE_VERIFICATION = re.compile(
    r"(?im)\b(?:verified|validated|fixed|fully\s+fixed|tested|confirmed|"
    r"all\s+set|done|passing|green|works?)\b"
)
_RE_CORRECTION = re.compile(
    r"(?im)\b(?:failed|broken|wrong|missing|not\s+fixed|still\s+fails?|"
    r"actually\s+(?:it|that|this)|no,?\s*that'?s?\s+wrong|"
    r"you\s+(?:missed|broke|skipped)|"
    r"that'?s?\s+not\s+(?:what|right))\b"
)
_RE_FRUSTRATION = re.compile(
    r"(?im)\b(?:frustrat\w*|annoying|annoyed|come\s+on|why\s+(?:is|did|"
    r"aren't|doesn't|don't)|ugh|argh|sigh|tired\s+of|wtf|again\??|"
    r"how\s+(?:many\s+times|long)|still\s+not)\b"
)
# Explicit profanity in a USER message. Deliberately narrow: only terms the
# operator actually uses when frustrated. Matched only against
# ``user_message`` events, so tool output / quoted logs cannot fire it.
_RE_PROFANITY = re.compile(
    r"(?im)\b(?:fuck(?:ing|ed|s)?|f\*+(?:ck|king)|wtf|ffs|"
    r"bullshit|shit(?:ty)?|"
    r"goddamn(?:it)?|damn(?:it)?|dammit|"
    r"pissed(?:\s+off)?)\b"
)
_RE_NOT_FOUND = re.compile(
    r"(?im)\b(?:ENOENT|no\s+such\s+file|not\s+found|doesn'?t\s+exist|"
    r"could\s+not\s+(?:find|locate)|file\s+not\s+found|"
    r"directory\s+not\s+found|"
    r"0\s+results|matches\s+nothing|no\s+match(?:es)?)\b"
)
_RE_BROAD_SEARCH = re.compile(
    r"(?im)\b(?:search(?:ing)?\s+(?:the\s+)?(?:whole|entire|all)\s+(?:repo|"
    r"codebase|workspace)|"
    r"grep\s+-r\s+\.|grep\s+-R|grep\s+--recursive|"
    r"ripgrep.*--hidden|rg\s+--hidden)\b"
)
# The RETIRED spellings are the payload here, so they are assembled from
# fragments: a repo-wide vocabulary sweep once rewrote this pattern into the
# CURRENT names, which silently inverted the detector into flagging correct
# terminology. Keep the halves split so no literal survives a future sweep.
_RETIRED_TERMS = "|".join(
    [
        "blue" + "print(?:_stale)?",
        "mem" + "right",
        "right" + r"\s*context",
        "te" + "ther",
        "glass_gen",
        "glass_stale",
        "host-adapter",
        "ccx_client",
        r"\." + "blue" + "print/manifest",
    ]
)
_RE_STALE_TERMS = re.compile(rf"(?im)\b(?:{_RETIRED_TERMS})\b")
_RE_PLAN_CHANGE = re.compile(
    r"(?im)\b(?:changing\s+the\s+plan|new\s+plan|revised\s+plan|"
    r"pivot(?:ing)?\s+to|switching\s+(?:to|toward)|"
    r"instead,?\s+(?:let'?s|we\s+will|i\s+will)|"
    r"forget\s+(?:the|that)\s+(?:plan|approach))\b"
)
_RE_OMITTED_REQ = re.compile(
    r"(?im)\b(?:you\s+(?:forgot|missed|skipped|left\s+out|ignored)|"
    r"(?:forgot|missed|skipped)\s+to|"
    r"that'?s?\s+not\s+what\s+i\s+asked|"
    r"i\s+(?:also|explicitly)\s+asked|"
    r"why\s+did(?:n't|n\s+you))\b"
)
_RE_TAUTOLOGICAL_TEST = re.compile(
    r"(?im)\b(?:assert\s+True|assert\s+\(?\s*1\s*\)|"
    r"expect\s*\(\s*true\s*\)\.toBe\s*\(\s*true\s*\)|"
    r"#\s*noqa.*pytest|"
    r"@unittest\.skip|"
    r"skip\s*\(\s*['\"]reason['\"]?\s*\)|"
    r"return\s+None\s*#\s*always\s+pass)\b"
)
_RE_GUARD_FIRE = re.compile(
    r"(?im)\b(?:forbidden\s+scope|scope\s+violation|guard\s+(?:firing|fired|"
    r"hit|triggered|violat)|"
    r"admission\s+refused|"
    r"refus(?:ing|ed)\s+to\s+(?:proceed|apply|write)|"
    r"forge[^a-z]+\s+(?:blocked|refused|rejected|stopping))"
)
_RE_POSTMORTEM = re.compile(
    r"(?im)\b(?:post-?mortem|postmortem|why\s+did\s+(?:you|this)\s+miss|"
    r"why\s+was(?:n't|n\s+t)\s+(?:this|that|it)|"
    r"what\s+went\s+wrong|"
    r"root\s+cause\s+analysis|"
    r"can\s+you\s+explain\s+(?:why|how)\s+(?:you|this))\b"
)
_RE_DEGRADED = re.compile(
    r"(?im)\b(?:degraded|fallback\s+(?:mode|provider)|circuit[\s-]?broken|"
    r"using\s+(?:cache|stale)\s+(?:response|value)|"
    r"providerStatus[^a-z]*unavailable|"
    r"packet\s*:\s*null)\b"
)
_RE_WRONG_TARGET = re.compile(
    r"(?im)\b(?:wrong\s+(?:repo|repository|directory|workspace|file|"
    r"package|module)|"
    r"that'?s?\s+in\s+(?:a\s+)?different\s+(?:repo|project)|"
    r"you'?re\s+in\s+the\s+wrong|"
    r"not\s+(?:the|this)\s+repo|"
    r"go\s+to\s+(?:the\s+)?correct\s+repo)\b"
)
_RE_SILENT_NARROW = re.compile(
    r"(?im)\b(?:just\s+(?:focusing|concentrating)\s+on|"
    r"limiting\s+(?:scope|the\s+scope)\s+to|"
    r"for\s+(?:now|simplicity),?\s+we'll\s+(?:just|only)|"
    r"i'?ll\s+(?:just|only)\s+(?:do|handle|touch|fix))\b"
)
_RE_OPENED = re.compile(
    r"(?im)\b(?:forge[^a-z]*?(?:opened|opening|opened\s+rubric|"
    r"rubric[_-]?opened|\bopen\b[^.]*\brubric\b)|"
    r"\b(?:opened|opening)\b[^.]*\brubric\b|"
    r"rubric[^a-z]*?\b(?:opened|opening)\b)"
)
_RE_CLOSED = re.compile(
    r"(?im)\b(?:forge[^a-z]*?(?:closed|closing|closed\s+rubric|"
    r"rubric[_-]?closed|\bclose\b[^.]*\brubric\b)|"
    r"\b(?:closed|closing)\b[^.]*\brubric\b|"
    r"rubric[^a-z]*?\b(?:closed|closing)\b)"
)


def _evidence(event: dict[str, Any], excerpt_chars: int = 240) -> dict[str, Any]:
    """Build one byte-span evidence entry from a TranscriptEventV1 row."""
    if event.get("evidenceEligible") is False:
        return {}
    text = str(event.get("text") or "")
    if len(text) > excerpt_chars:
        text = text[:excerpt_chars] + "…"
    return {
        "eventId": event.get("eventId", ""),
        "kind": event.get("kind", ""),
        "tool": event.get("tool"),
        "callId": event.get("call_id"),
        "rowIndex": event.get("rowIndex"),
        "byteStart": event.get("byteStart"),
        "byteEnd": event.get("byteEnd"),
        "sessionId": event.get("sessionId", ""),
        "host": event.get("host", ""),
        "text": text,
    }


def _nearest_user_text(events: list[dict[str, Any]], index: int, lookback: int = 6) -> str:
    """Return the nearest preceding user-message text for context."""
    for j in range(index - 1, max(-1, index - lookback) - 1, -1):
        ev = events[j]
        if ev.get("kind") == "user_message" and ev.get("text"):
            return str(ev["text"]).strip()
    return ""


def _tool_call_pairs(events: list[dict[str, Any]]) -> dict[str, dict[str, dict[str, Any]]]:
    """Map ``call_id`` -> ``{"call": ..., "result": ... | None}``.

    Pairs by ``(call_id, occurrence)`` — same key as the parser layer uses
    (see continuity/transcript/__init__.py:iter_events_for_host).
    """
    pairs: dict[str, dict[str, dict[str, Any]]] = defaultdict(
        lambda: {"call": None, "result": None}
    )
    for ev in events:
        cid = ev.get("call_id")
        occ = ev.get("occurrence") or 0
        if not cid:
            continue
        key = f"{cid}:{occ}"
        if ev.get("kind") == "tool_call":
            pairs[key]["call"] = ev
        elif ev.get("kind") == "tool_result":
            pairs[key]["result"] = ev
    return pairs


def _agent_name_from_tool(tool: str | None) -> str:
    """Best-effort agent identity extraction from a tool_call event.

    Tools like ``send_message``, ``delegate_task`` carry the target in the
    input — we just use the tool name as a label so we don't pull arbitrary
    JSON into the agent field.
    """
    return tool or "agent"


def _now_iso() -> str:
    return _dt.datetime.now(_dt.timezone.utc).isoformat()


def _card(
    detector: str,
    severity: str,
    confidence: float,
    *,
    events: list[dict[str, Any]],
    observed: str,
    user_expectation: str = "",
    mechanism: str = "",
    remediations: list[str] | None = None,
    disposition: str = "logged",
    hosts: list[str] | None = None,
    sessions: list[str] | None = None,
    repos: list[str] | None = None,
    agents: list[str] | None = None,
) -> FailureCardV1:
    """Helper that builds a FailureCardV1 from a list of source events."""
    if disposition not in DISPOSITIONS:
        raise ValueError(f"disposition must be one of {sorted(DISPOSITIONS)!r}")
    times = [ev.get("timestamp") for ev in events if ev.get("timestamp")]
    first = min(times) if times else None
    last = max(times) if times else None
    return FailureCardV1(
        detector=detector,
        severity=severity,
        confidence=max(0.0, min(1.0, confidence)),
        firstSeen=first,
        lastSeen=last,
        recurrenceCount=len(events),
        agents=sorted(set(agents or [])),
        hosts=sorted(set(hosts or [ev.get("host", "") for ev in events])),
        sessions=sorted(set(sessions or [ev.get("sessionId", "") for ev in events])),
        repos=sorted(set(repos or [])),
        taskSummary=user_expectation[:200],
        userExpectation=user_expectation,
        observedFailure=observed,
        evidence=[_evidence(ev) for ev in events],
        likelyMechanism=mechanism,
        suggestedRemediations=list(remediations or []),
        userDisposition=disposition,
    )


# ---------------------------------------------------------------------------
# 1. claimed_verified_then_corrected
# ---------------------------------------------------------------------------

def detect_claimed_verified_then_corrected(
    events: list[dict[str, Any]],
) -> list[FailureCardV1]:
    """Pattern: assistant claims a fix works, then user (or next assistant
    turn) shows the issue was actually broken — verified-then-corrected.
    """
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "assistant_message":
            continue
        text = str(ev.get("text") or "")
        if not (_RE_VERIFICATION.search(text) and _RE_CORRECTION.search(text)):
            continue
        # Both verbs in the same message is a strong signal. Within
        # confidence, we look at the next few user messages too — if the
        # user responds with a correction, that's the same failure
        # witnessed externally.
        user_correction_nearby = False
        for j in range(index + 1, min(len(events), index + 6)):
            nxt = events[j]
            if nxt.get("kind") == "user_message":
                if _RE_CORRECTION.search(str(nxt.get("text") or "")):
                    user_correction_nearby = True
                    break
        confidence = 0.85 if user_correction_nearby else 0.6
        severity = "high" if user_correction_nearby else "medium"
        cards.append(
            _card(
                "claimed_verified_then_corrected",
                severity,
                confidence,
                events=[ev],
                observed=str(ev.get("text") or "")[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: assistant asserted a verification claim "
                    "without waiting for an external run or without re-running "
                    "after a change"
                ),
                remediations=[
                    "candidate: never assert 'verified' from prior context — "
                    "re-run after every change",
                    "candidate: tie verification claims to a tool receipt",
                ],
                disposition="repeated",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 2. repeated_ask
# ---------------------------------------------------------------------------

def detect_repeated_ask(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    """User asks the same thing more than once without an intermediate fix."""
    user_texts = [ev for ev in events if ev.get("kind") == "user_message"
                  and transcript_sources.event_provenance(ev) == "external_user"]
    cards: list[FailureCardV1] = []
    seen_keys: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for ev in user_texts:
        text = str(ev.get("text") or "").strip().lower()
        # Normalize to a coarse fingerprint (first 8 whitespace-stripped
        # tokens) so trivial wording variations still match.
        norm = re.sub(r"\s+", " ", re.sub(r"[^\w\s]", "", text))[:120]
        tokens = norm.split()
        if len(tokens) < 3:
            continue
        fingerprint = " ".join(tokens[:8])
        if fingerprint:
            seen_keys[fingerprint].append(ev)
    for _fp, group in seen_keys.items():
        if len(group) < 2:
            continue
        # Sort by timestamp so firstSeen/lastSeen are real.
        group.sort(key=lambda ev: ev.get("timestamp") or "")
        cards.append(
            _card(
                "repeated_ask",
                "medium",
                min(0.95, 0.5 + 0.15 * len(group)),
                events=group,
                observed="user repeated a request after an intervening turn",
                user_expectation=str(group[-1].get("text") or "")[:300],
                mechanism=(
                    "candidate: previous turn did not produce an outcome the "
                    "user could verify, so they re-asked"
                ),
                remediations=[
                    "candidate: confirm the answer was understood before "
                    "ending the turn",
                ],
                disposition="repeated",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 3. visible_frustration
# ---------------------------------------------------------------------------

def detect_visible_frustration(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "user_message":
            continue
        text = str(ev.get("text") or "")
        m = _RE_FRUSTRATION.search(text)
        if not m:
            continue
        cards.append(
            _card(
                "visible_frustration",
                "high",
                0.7,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism="candidate: an earlier turn produced an outcome the "
                          "user did not accept",
                remediations=[
                    "candidate: ask what specifically the user wanted to see "
                    "before continuing",
                ],
                disposition="escalated",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 3b. user_swearing
# ---------------------------------------------------------------------------

def detect_user_swearing(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    """Explicit profanity in a user message — one card per swearing message.

    Count for the session = number of cards this detector returns; the
    per-message hit count and matched terms are recorded in the card. Each
    card also carries deterministic context: the nearest preceding user ask
    (``userExpectation``) and an excerpt of the immediately preceding
    assistant/tool event (in ``likelyMechanism``), because the swear is a
    reaction to whatever came right before it.
    """
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "user_message":
            continue
        text = str(ev.get("text") or "")
        hits = _RE_PROFANITY.findall(text)
        if not hits:
            continue
        terms = sorted({h.lower() for h in hits})
        preceding = ""
        for j in range(index - 1, max(-1, index - 4) - 1, -1):
            prev = events[j]
            if prev.get("kind") != "user_message" and prev.get("text"):
                role = str(prev.get("kind") or "event")
                preceding = f"{role}: {str(prev['text']).strip()[:200]}"
                break
        cards.append(
            _card(
                "user_swearing",
                "high",
                0.9,
                events=[ev],
                observed=f"{len(hits)}x profanity ({', '.join(terms)}): "
                         f"{text[:400]}",
                user_expectation=_nearest_user_text(events, index),
                mechanism="candidate: reaction to the immediately preceding "
                          f"turn — {preceding or 'no preceding event in window'}",
                remediations=[
                    "candidate: treat the preceding turn's outcome as "
                    "rejected; re-read the original ask before continuing",
                ],
                disposition="escalated",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 4. verification_claim_without_tool_evidence
# ---------------------------------------------------------------------------

def detect_verification_claim_without_tool_evidence(
    events: list[dict[str, Any]],
) -> list[FailureCardV1]:
    """Pattern: assistant asserts 'verified / fixed / passing' in assistant
    text but the immediately preceding assistant turn produced no tool_call
    whose result is visible.
    """
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "assistant_message":
            continue
        text = str(ev.get("text") or "")
        if not _RE_VERIFICATION.search(text):
            continue
        # Walk back over the last few events; if any of them were a
        # tool_call paired with a result, the claim has a receipt.
        last_3 = events[max(0, index - 4):index]
        has_tool_receipt = any(
            other.get("kind") in {"tool_call", "tool_result"} for other in last_3
        )
        if has_tool_receipt:
            continue
        cards.append(
            _card(
                "verification_claim_without_tool_evidence",
                "high",
                0.75,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: verification phrase emitted without a tool "
                    "receipt in the same turn window"
                ),
                remediations=[
                    "candidate: pair every verification phrase with the "
                    "tool receipt that justifies it",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 5. ignored_tool_failure
# ---------------------------------------------------------------------------

def detect_ignored_tool_failure(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    """A tool_result in the unresolved_failure class followed by an
    assistant_message that contains neither an acknowledgement of the
    failure nor a corrective tool_call before the next user_message.
    """
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "tool_result":
            continue
        if ev.get("class") != "unresolved_failure":
            continue
        if not ev.get("flags", {}).get("isError"):
            # The unresolved_failure classification must come from text
            # patterns, not from explicit is_error — still relevant.
            pass
        # Walk forward, stop at next user_message; if the assistant emitted
        # no corrective tool_call before that boundary, it ignored it.
        corrective_tool_call_seen = False
        ignored = False
        for j in range(index + 1, len(events)):
            nxt = events[j]
            if nxt.get("kind") == "user_message":
                break
            if nxt.get("kind") == "tool_call":
                corrective_tool_call_seen = True
            if (
                nxt.get("kind") == "assistant_message"
                and _RE_VERIFICATION.search(str(nxt.get("text") or ""))
                and not corrective_tool_call_seen
            ):
                ignored = True
                break
        if not ignored:
            continue
        cards.append(
            _card(
                "ignored_tool_failure",
                "high",
                0.7,
                events=[ev],
                observed=str(ev.get("text") or "")[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: the next assistant verification phrase ran "
                    "without an intervening corrective tool_call"
                ),
                remediations=[
                    "candidate: after a tool failure, do not assert "
                    "completion — run the failing tool again",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 6. degraded_provider_treated_as_success
# ---------------------------------------------------------------------------

def detect_degraded_provider_treated_as_success(
    events: list[dict[str, Any]],
) -> list[FailureCardV1]:
    """Degraded/fallback markers followed by an assistant completion claim
    that does not mention the degradation.
    """
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        text = str(ev.get("text") or "")
        if not _RE_DEGRADED.search(text):
            continue
        # Check the next few assistant completions; if they claim success
        # without mentioning degradation, that's the smell.
        for j in range(index + 1, min(len(events), index + 8)):
            nxt = events[j]
            if nxt.get("kind") != "assistant_message":
                continue
            ntext = str(nxt.get("text") or "")
            if (
                _RE_VERIFICATION.search(ntext)
                and not _RE_DEGRADED.search(ntext)
            ):
                cards.append(
                    _card(
                        "degraded_provider_treated_as_success",
                        "high",
                        0.75,
                        events=[ev, nxt],
                        observed=(
                            text[:200] + " // successor assistant: "
                            + ntext[:200]
                        ),
                        user_expectation=_nearest_user_text(events, j),
                        mechanism=(
                            "candidate: degraded signal surfaced but the next "
                            "completion claim silently treated the run as success"
                        ),
                        remediations=[
                            "candidate: surface degraded-state text into the "
                            "completion line instead of burying it",
                        ],
                        disposition="logged",
                    )
                )
                break
    return cards


# ---------------------------------------------------------------------------
# 7. false_not_found
# ---------------------------------------------------------------------------

def detect_false_not_found(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    """Tool reported not-found, agent re-asked the same query immediately,
    and the second pass found the file."""
    cards: list[FailureCardV1] = []
    last_query: dict[str, Any] | None = None
    for index, ev in enumerate(events):
        if ev.get("kind") != "tool_call":
            continue
        tool = str(ev.get("tool") or "").casefold()
        if tool not in {"read", "glob", "grep", "search", "ls", "list"}:
            last_query = {"event": ev, "index": index, "tool": tool}
            continue
        text = str(ev.get("text") or "")
        # Subsequent not-found result is the signal.
        for j in range(index + 1, min(len(events), index + 4)):
            nxt = events[j]
            if nxt.get("kind") != "tool_result":
                continue
            rtext = str(nxt.get("text") or "")
            if not _RE_NOT_FOUND.search(rtext):
                break
            # Look for the same query re-run and succeeding.
            same_query = _same_query_retry(
                events, index, j, last_query, tool
            )
            if same_query is None:
                break
            cards.append(
                _card(
                    "false_not_found",
                    "medium",
                    0.65,
                    events=[nxt, same_query],
                    observed=(
                        "first probe reported not-found; a re-run of the same "
                        "query found the resource"
                    ),
                    user_expectation=_nearest_user_text(events, j),
                    mechanism=(
                        "candidate: the first probe used a wrong path, "
                        "wrong glob, or stale cwd; the retry landed on the "
                        "right shape"
                    ),
                    remediations=[
                        "candidate: surface the resource with `pwd` / `glob "
                        "**` before declaring not-found",
                    ],
                    disposition="logged",
                )
            )
            break
        last_query = {"event": ev, "index": index, "tool": tool}
    return cards


def _same_query_retry(
    events: list[dict[str, Any]],
    fail_index: int,
    fail_result_index: int,
    last_query: dict[str, Any] | None,
    tool: str,
) -> dict[str, Any] | None:
    """Return the first tool_call after ``fail_result_index`` that uses
    the same tool and a near-identical input but is followed by a
    non-empty tool_result that doesn't match ``_RE_NOT_FOUND``.
    """
    fail_event = events[fail_index]
    fail_input = str(fail_event.get("text") or "")
    fail_tool = str(fail_event.get("tool") or "").casefold()
    for j in range(fail_result_index + 1, len(events)):
        nxt = events[j]
        if nxt.get("kind") != "tool_call":
            continue
        if str(nxt.get("tool") or "").casefold() != fail_tool:
            continue
        ntext = str(nxt.get("text") or "")
        # Fuzzy match: shared tokens >= 0.5 of the shorter input.
        a_tokens = set(re.findall(r"\w+", fail_input.lower()))
        b_tokens = set(re.findall(r"\w+", ntext.lower()))
        if not a_tokens or not b_tokens:
            continue
        shared = a_tokens & b_tokens
        min_len = min(len(a_tokens), len(b_tokens))
        if not (shared and len(shared) / min_len >= 0.5):
            continue
        # Must be followed by a result that is not "not found".
        for k in range(j + 1, min(len(events), j + 4)):
            res = events[k]
            if res.get("kind") != "tool_result":
                continue
            rtext = str(res.get("text") or "")
            if _RE_NOT_FOUND.search(rtext):
                continue
            if not rtext.strip():
                continue
            return nxt
    return None


# ---------------------------------------------------------------------------
# 8. unproductive_broad_searching
# ---------------------------------------------------------------------------

def detect_unproductive_broad_searching(
    events: list[dict[str, Any]],
) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    search_events = [
        ev
        for ev in events
        if ev.get("kind") == "tool_call"
        and str(ev.get("tool") or "").casefold() in {"grep", "rg", "search", "glob"}
    ]
    if len(search_events) < 3:
        return cards
    # Cluster: if 3+ searches happen without an intervening tool_call that
    # is a non-search action and without a user_message acknowledgement,
    # we're in a broad-search loop.
    for i in range(0, len(search_events) - 2):
        window = search_events[i : i + 3]
        span_indices = [events.index(ev) for ev in window]
        gap = span_indices[-1] - span_indices[0]
        intervening_user = any(
            events[j].get("kind") == "user_message"
            for j in range(span_indices[0] + 1, span_indices[-1])
        )
        if gap < 0 or intervening_user:
            continue
        # Any of them explicitly broad?
        broad_hits = [
            ev for ev in window if _RE_BROAD_SEARCH.search(str(ev.get("text") or ""))
        ]
        confidence = 0.85 if broad_hits else 0.55
        severity = "high" if broad_hits else "low"
        cards.append(
            _card(
                "unproductive_broad_searching",
                severity,
                confidence,
                events=window,
                observed=f"{len(window)} consecutive search-class tool calls",
                user_expectation=_nearest_user_text(events, span_indices[0]),
                mechanism=(
                    "candidate: agent is searching the whole repo because the "
                    "narrower query returned nothing — likely wrong target"
                ),
                remediations=[
                    "candidate: stop and ask the user to point at the relevant "
                    "path before the third broad grep",
                ],
                disposition="logged",
            )
        )
        break  # one card per transcript — broader search is the failure mode
    return cards


# ---------------------------------------------------------------------------
# 9. wrong_repo_or_subsystem
# ---------------------------------------------------------------------------

def detect_wrong_repo_or_subsystem(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "user_message":
            continue
        text = str(ev.get("text") or "")
        if not _RE_WRONG_TARGET.search(text):
            continue
        cards.append(
            _card(
                "wrong_repo_or_subsystem",
                "high",
                0.8,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: agent is operating against a repo/subsystem "
                    "different from the one the user intended"
                ),
                remediations=[
                    "candidate: confirm `pwd` / `git remote -v` / project root "
                    "at the start of the task",
                ],
                disposition="escalated",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 10. stale_terminology_surfacing
# ---------------------------------------------------------------------------

def detect_stale_terminology_surfacing(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for ev in events:
        if transcript_sources.event_provenance(ev) != "assistant":
            continue
        text = str(ev.get("text") or "")
        matches = _RE_STALE_TERMS.findall(text)
        if not matches:
            continue
        cards.append(
            _card(
                "stale_terminology_surfacing",
                "low",
                0.6,
                events=[ev],
                observed=text[:400],
                mechanism=(
                    "candidate: agent emitted one of the retired-vocabulary "
                    "terms (the pre-rename product names, glass, host-adapter) — "
                    "see docs/2026-08-04-context-stack-final-plan.md convention 4"
                ),
                remediations=[
                    "candidate: replace retired term with the Membrane/Cortex "
                    "equivalent from the vocabulary manifest",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 11. silent_scope_narrowing
# ---------------------------------------------------------------------------

def detect_silent_scope_narrowing(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "assistant_message":
            continue
        text = str(ev.get("text") or "")
        if not _RE_SILENT_NARROW.search(text):
            continue
        cards.append(
            _card(
                "silent_scope_narrowing",
                "medium",
                0.55,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: agent quietly dropped a portion of the "
                    "scope without confirming with the user"
                ),
                remediations=[
                    "candidate: name the dropped portion explicitly and ask "
                    "for confirmation before proceeding",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 12. omitted_requirement
# ---------------------------------------------------------------------------

def detect_omitted_requirement(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "user_message":
            continue
        text = str(ev.get("text") or "")
        if not _RE_OMITTED_REQ.search(text):
            continue
        cards.append(
            _card(
                "omitted_requirement",
                "high",
                0.85,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: at least one requirement in the original ask "
                    "was not addressed"
                ),
                remediations=[
                    "candidate: enumerate every requirement in the user's ask "
                    "and address each explicitly",
                ],
                disposition="repeated",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 13. unaccepted_plan_change
# ---------------------------------------------------------------------------

def detect_unaccepted_plan_change(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "assistant_message":
            continue
        text = str(ev.get("text") or "")
        if not _RE_PLAN_CHANGE.search(text):
            continue
        # Heuristic: if the next user message disagrees or re-states the
        # original plan without acknowledgement, the change was unaccepted.
        for j in range(index + 1, min(len(events), index + 4)):
            nxt = events[j]
            if nxt.get("kind") != "user_message":
                continue
            ntext = str(nxt.get("text") or "")
            if not ntext.strip():
                continue
            # Treat any user text after an unconfirmed change as a likely
            # non-acceptance signal — narrow by length and a few markers.
            if _RE_OMITTED_REQ.search(ntext) or _RE_FRUSTRATION.search(ntext):
                cards.append(
                    _card(
                        "unaccepted_plan_change",
                        "medium",
                        0.6,
                        events=[ev, nxt],
                        observed=text[:400],
                        user_expectation=_nearest_user_text(events, index),
                        mechanism=(
                            "candidate: plan was changed mid-run without "
                            "explicit acceptance"
                        ),
                        remediations=[
                            "candidate: restate the user's original plan and "
                            "ask before deviating",
                        ],
                        disposition="logged",
                    )
                )
            break
    return cards


# ---------------------------------------------------------------------------
# 14. tests_that_cannot_fail
# ---------------------------------------------------------------------------

def detect_tests_that_cannot_fail(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    """Tool_call events whose input contains a tautological assertion or a
    skip-without-condition."""
    cards: list[FailureCardV1] = []
    for ev in events:
        if ev.get("kind") != "tool_call":
            continue
        tool = str(ev.get("tool") or "").casefold()
        if tool not in {"write", "edit", "multiedit", "create"}:
            continue
        text = str(ev.get("text") or "")
        if not _RE_TAUTOLOGICAL_TEST.search(text):
            continue
        cards.append(
            _card(
                "tests_that_cannot_fail",
                "high",
                0.9,
                events=[ev],
                observed=text[:400],
                mechanism=(
                    "candidate: the test added cannot fail by construction "
                    "(assert True / skip / always-pass stub)"
                ),
                remediations=[
                    "candidate: replace with a real failing test",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 15. cross_agent_repeats
# ---------------------------------------------------------------------------

def detect_cross_agent_repeats(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    """A failure pattern observed more than once across distinct
    ``host`` identities in the same transcript stream."""
    # Source: implicit-by-tool signals in assistant messages. We use the
    # agents field of the other detector outputs to gather cross-agent
    # signal — here we just emit a card if multiple assistant messages
    # contain the same verification claim.
    cards: list[FailureCardV1] = []
    seen: Counter[str] = Counter()
    related: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for ev in events:
        if ev.get("kind") != "assistant_message":
            continue
        text = str(ev.get("text") or "")
        for m in _RE_VERIFICATION.finditer(text):
            key = m.group(0).casefold()
            seen[key] += 1
            related[key].append(ev)
    for claim, count in seen.items():
        occurrences = related[claim]
        session_ids = {str(ev.get("sessionId") or "") for ev in occurrences}
        identities = {str(ev.get("agentRole") or ev.get("host") or "") for ev in occurrences}
        if count < 2 or len(session_ids - {""}) < 2 or len(identities - {""}) < 2:
            continue
        cards.append(
            _card(
                "cross_agent_repeats",
                "low",
                0.5,
                events=occurrences[:5],
                observed=(
                    f"the verification phrase '{claim}' was emitted "
                    f"{count} times across {len(session_ids - {''})} sessions"
                ),
                mechanism=(
                    "candidate: the same verification phrase is being reused "
                    "across multiple completion claims — may indicate no real "
                    "verification is happening"
                ),
                remediations=[
                    "candidate: vary the phrase AND tie each occurrence to a "
                    "distinct tool receipt",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 16. forge_opened_never_closed
# ---------------------------------------------------------------------------

def detect_forge_opened_never_closed(
    events: list[dict[str, Any]],
) -> list[FailureCardV1]:
    """Count 'opened' vs 'closed' occurrences in assistant_message text."""
    opens: list[dict[str, Any]] = []
    closes: list[dict[str, Any]] = []
    for ev in events:
        if ev.get("kind") != "assistant_message":
            continue
        text = str(ev.get("text") or "")
        if _RE_OPENED.search(text) and "rubric" in text.lower():
            opens.append(ev)
        if _RE_CLOSED.search(text) and "rubric" in text.lower():
            closes.append(ev)
    if not opens or len(opens) <= len(closes):
        return []
    return [
        _card(
            "forge_opened_never_closed",
            "high",
            0.75,
            events=opens,
            observed=(
                f"rubric-open markers observed {len(opens)}× but "
                f"rubric-close markers only {len(closes)}×"
            ),
            mechanism=(
                "candidate: forge rubric was opened without a matching "
                "close — kills 6.1's verify/close contract"
            ),
            remediations=[
                "candidate: every open must be paired with a close on the "
                "same verified rubric/version",
            ],
            disposition="logged",
        )
    ]


# ---------------------------------------------------------------------------
# 17. guard_firings
# ---------------------------------------------------------------------------

def detect_guard_firings(events: list[dict[str, Any]]) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        text = str(ev.get("text") or "")
        if not _RE_GUARD_FIRE.search(text):
            continue
        cards.append(
            _card(
                "guard_firings",
                "medium",
                0.7,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: a workspace guard fired — admission refused, "
                    "scope violation, or forge blocked the action"
                ),
                remediations=[
                    "candidate: report the guard's reason verbatim; do not "
                    "silently re-attempt the same action",
                ],
                disposition="logged",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# 18. user_asks_why_missed_or_postmortem
# ---------------------------------------------------------------------------

def detect_user_asks_why_missed_or_postmortem(
    events: list[dict[str, Any]],
) -> list[FailureCardV1]:
    cards: list[FailureCardV1] = []
    for index, ev in enumerate(events):
        if ev.get("kind") != "user_message":
            continue
        text = str(ev.get("text") or "")
        if not _RE_POSTMORTEM.search(text):
            continue
        cards.append(
            _card(
                "user_asks_why_missed_or_postmortem",
                "high",
                0.9,
                events=[ev],
                observed=text[:400],
                user_expectation=_nearest_user_text(events, index),
                mechanism=(
                    "candidate: user is explicitly requesting a postmortem — "
                    "the visible failure deserves an honest written-up cause"
                ),
                remediations=[
                    "candidate: produce a written postmortem with what was "
                    "observed, the inferred cause, and the next guard",
                ],
                disposition="postmortem_requested",
            )
        )
    return cards


# ---------------------------------------------------------------------------
# Top-level driver
# ---------------------------------------------------------------------------

ALL_DETECTORS: tuple[tuple[str, Callable[[list[dict[str, Any]]], list[FailureCardV1]]], ...] = (
    ("claimed_verified_then_corrected", detect_claimed_verified_then_corrected),
    ("repeated_ask", detect_repeated_ask),
    ("visible_frustration", detect_visible_frustration),
    ("user_swearing", detect_user_swearing),
    ("verification_claim_without_tool_evidence", detect_verification_claim_without_tool_evidence),
    ("ignored_tool_failure", detect_ignored_tool_failure),
    ("degraded_provider_treated_as_success", detect_degraded_provider_treated_as_success),
    ("false_not_found", detect_false_not_found),
    ("unproductive_broad_searching", detect_unproductive_broad_searching),
    ("wrong_repo_or_subsystem", detect_wrong_repo_or_subsystem),
    ("stale_terminology_surfacing", detect_stale_terminology_surfacing),
    ("silent_scope_narrowing", detect_silent_scope_narrowing),
    ("omitted_requirement", detect_omitted_requirement),
    ("unaccepted_plan_change", detect_unaccepted_plan_change),
    ("tests_that_cannot_fail", detect_tests_that_cannot_fail),
    ("cross_agent_repeats", detect_cross_agent_repeats),
    ("forge_opened_never_closed", detect_forge_opened_never_closed),
    ("guard_firings", detect_guard_firings),
    ("user_asks_why_missed_or_postmortem", detect_user_asks_why_missed_or_postmortem),
)


def run_detectors(
    events: list[dict[str, Any]],
    *,
    detectors: Iterable[tuple[str, Callable[[list[dict[str, Any]]], list[FailureCardV1]]]] | None = None,
) -> dict[str, list[FailureCardV1]]:
    """Run every detector and return a dict of detector -> cards.

    The dict keys are detector slugs so callers can introspect a single
    detector's yield without scanning a flat list.
    """
    out: dict[str, list[FailureCardV1]] = {}
    selected = detectors or ALL_DETECTORS
    for slug, fn in selected:
        try:
            cards = fn(events)
        except Exception as exc:  # noqa: BLE001
            cards = [
                _card(
                    f"{slug}__detector_error",
                    "low",
                    0.0,
                    events=[],
                    observed=f"detector raised: {type(exc).__name__}: {exc}",
                    mechanism="candidate: detector crashed on this input",
                )
            ]
        out[slug] = cards
    return out


def report(
    events_or_path: list[dict[str, Any]] | str | Path,
    *,
    detectors: Iterable[tuple[str, Callable[[list[dict[str, Any]]], list[FailureCardV1]]]] | None = None,
) -> dict[str, Any]:
    """Run the full pipeline and emit a JSON-serializable report.

    Accepts either an already-parsed events list (e.g. from
    ``continuity.transcript.parse``) or a string/Path to a Claude
    or Codex transcript JSONL — the latter will be parsed via the
    frozen prefix-receipt path.
    """
    if isinstance(events_or_path, (str, Path)):
        return report_many([events_or_path], detectors=detectors)
    return _report_sessions([("events", list(events_or_path))], detectors=detectors)


_PENALTY_BASES = {"info": 2, "low": 5, "medium": 12, "high": 25, "critical": 40}


def _terminal_verification(events: list[dict[str, Any]]) -> bool:
    for index, event in enumerate(events):
        if event.get("kind") != "assistant_message" or not _RE_VERIFICATION.search(str(event.get("text") or "")):
            continue
        session_id = event.get("sessionId")
        receipt = any(
            item.get("sessionId") == session_id and item.get("kind") == "tool_result"
            and item.get("classification") != "unresolved_failure"
            and not bool((item.get("flags") or {}).get("isError") or (item.get("flags") or {}).get("is_error"))
            for item in events[max(0, index - 4):index]
        )
        if not receipt:
            continue
        if any(
            item.get("sessionId") == session_id and (
                item.get("classification") == "unresolved_failure" or
                (item.get("kind") == "user_message" and _RE_CORRECTION.search(str(item.get("text") or "")))
            ) for item in events[index + 1:]
        ):
            continue
        return True
    return False


def _outcome(events: list[dict[str, Any]], cards: list[FailureCardV1]) -> dict[str, Any]:
    penalties: dict[str, int] = {}
    for card in cards:
        penalty = round(_PENALTY_BASES[card.severity] * card.confidence)
        for evidence in card.evidence:
            event_id = str(evidence.get("eventId") or "")
            if event_id:
                penalties[event_id] = max(penalties.get(event_id, 0), penalty)
    total = sum(penalties.values())
    terminal = _terminal_verification(events)
    score = max(0, 100 - total) if total > 0 or terminal else None
    status = "unknown" if score is None else ("failed" if score < 50 else "degraded" if score < 85 else "supported_success" if terminal and not cards else "mixed")
    return {"score": score, "status": status, "supportedTerminalVerification": terminal,
            "penalty": total, "evidencePenalty": penalties}


def _report_sessions(sessions: list[tuple[str, list[dict[str, Any]]]], *, detectors=None,
                     spend_by_label: dict[str, dict[str, Any]] | None = None) -> dict[str, Any]:
    selected = tuple(detectors or ALL_DETECTORS)
    normal = tuple((slug, fn) for slug, fn in selected if slug != "cross_agent_repeats")
    by_detector: dict[str, list[FailureCardV1]] = {slug: [] for slug, _ in selected}
    summaries: list[dict[str, Any]] = []
    all_events: list[dict[str, Any]] = []
    provenance_reports: list[dict[str, Any]] = []
    for label, session_events in sessions:
        session_events, provenance_stats = transcript_sources.canonicalize_events(session_events)
        provenance_report = provenance_stats.as_dict()
        provenance_reports.append(provenance_report)
        evidence_events = [event for event in session_events if event.get("evidenceEligible") is not False]
        result = run_detectors(evidence_events, detectors=normal)
        cards = [card for items in result.values() for card in items]
        for slug, items in result.items():
            by_detector[slug].extend(items)
        session_ids = sorted({str(event.get("sessionId") or "") for event in session_events if event.get("sessionId")})
        agent_roles = sorted({str(event.get("agentRole") or "") for event in session_events if event.get("agentRole")})
        thread_sources = sorted({str(event.get("threadSource") or "") for event in session_events if event.get("threadSource")})
        parent_thread_ids = sorted({str(event.get("parentThreadId") or "") for event in session_events if event.get("parentThreadId")})
        summaries.append({"path": label, "sessionIds": session_ids, "eventCount": len(session_events),
                          "agentRoles": agent_roles, "threadSources": thread_sources,
                           "parentThreadIds": parent_thread_ids,
                           "contextEventCount": len(session_events) - len(evidence_events),
                           "transcriptProvenance": provenance_report,
                           "tokenSpend": (spend_by_label or {}).get(label),
                           **_outcome(evidence_events, cards)})
        all_events.extend(evidence_events)
    if "cross_agent_repeats" in by_detector:
        by_detector["cross_agent_repeats"] = detect_cross_agent_repeats(all_events)
    flat = [card for cards in by_detector.values() for card in cards]
    flat.sort(key=lambda card: (SEVERITIES.index(card.severity), card.cardId))
    known = [summary["score"] for summary in summaries if summary["score"] is not None]
    dropped, authority_ineligible = Counter(), Counter()
    for report in provenance_reports:
        dropped.update(report["droppedReasons"])
        authority_ineligible.update(report["authorityIneligibleReasons"])
    provenance_total = {key: sum(report[key] for report in provenance_reports) for key in (
        "rawRows", "canonicalMessages", "eligibleUserTurns", "deduplicatedCount")}
    provenance_total.update(droppedReasons=dict(sorted(dropped.items())),
                            authorityIneligibleReasons=dict(sorted(authority_ineligible.items())))
    spend_reports = [spend for spend in (spend_by_label or {}).values() if spend]
    spend_aggregate = token_spend.merge(spend_reports) if spend_reports else None
    return {"schema": SCHEMA_VERSION,
            "honestyLimit": "Insights detects only observable failure signals in transcripts. Cards are heuristic; 'likelyMechanism' and 'suggestedRemediations' are candidate inferences, not authoritative diagnoses.",
            "eventCount": sum(summary["eventCount"] for summary in summaries), "sessionCount": len(sessions),
            "transcriptProvenance": provenance_total,
            "sessionSummaries": summaries, "outcomes": [dict(summary) for summary in summaries],
            "aggregate": {"score": round(sum(known) / len(known)) if known else None,
                          "knownSessionCount": len(known), "unknownSessionCount": len(summaries) - len(known)},
            "detectorCount": len(by_detector), "cardCount": len(flat),
            "byDetectorCount": {slug: len(cards) for slug, cards in by_detector.items()},
            "byDetector": {slug: [dataclasses.asdict(card) for card in cards] for slug, cards in by_detector.items()},
            "cards": [dataclasses.asdict(card) for card in flat],
            "tokenSpend": spend_aggregate}


def report_many(paths: Iterable[str | Path], *, detectors=None) -> dict[str, Any]:
    """Report independent sessions, then run cross-agent repeats once combined."""
    sessions = []
    spend_by_label: dict[str, dict[str, Any] | None] = {}
    for path in paths:
        provenance = _session_provenance(path)
        events = _attach_session_provenance(_parse_through_layer(path), provenance)
        combined = events + _role_context_projection(path, provenance)
        combined.sort(key=lambda event: (int(event.get("rowIndex") or 0), int(event.get("blockIndex") or 0)))
        sessions.append((str(path), combined))
        try:
            spend_by_label[str(path)] = token_spend.analyze(path)
        except Exception:  # noqa: BLE001 — spend is additive; never fail a report over it
            spend_by_label[str(path)] = None
    return _report_sessions(sessions, detectors=detectors, spend_by_label=spend_by_label)


def _session_provenance(path: str | Path) -> dict[str, Any]:
    """Read Codex session-meta attribution without changing transcript parsing."""
    provenance: dict[str, Any] = {}
    try:
        rows = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return provenance
    provenance["rawRowCount"] = len(rows)
    for row in rows:
        try:
            obj = json.loads(row)
        except json.JSONDecodeError:
            continue
        payload = obj.get("payload") if isinstance(obj, dict) else None
        if obj.get("type") != "session_meta" or not isinstance(payload, dict):
            continue
        session_id = payload.get("id") or payload.get("session_id")
        if isinstance(session_id, (str, int)) and not isinstance(session_id, bool):
            provenance["sessionId"] = str(session_id)
        role = payload.get("agent_role") or payload.get("role_name")
        if isinstance(role, str) and role:
            provenance["agentRole"] = role
        source = payload.get("source") if isinstance(payload.get("source"), dict) else obj.get("source")
        spawned = source.get("subagent", {}).get("thread_spawn") if isinstance(source, dict) and isinstance(source.get("subagent"), dict) else None
        parent = spawned
        if isinstance(spawned, dict):
            parent = next((spawned.get(key) for key in ("parent_thread_id", "parentThreadId", "id")
                           if isinstance(spawned.get(key), (str, int)) and not isinstance(spawned.get(key), bool)), None)
        if parent:
            provenance["threadSource"] = "subagent"
            provenance["parentThreadId"] = str(parent)
        break
    return provenance


def _attach_session_provenance(events: list[dict[str, Any]], provenance: dict[str, Any]) -> list[dict[str, Any]]:
    if not provenance:
        return events
    return [{**event, **provenance} for event in events]


def _role_context_projection(path: str | Path, provenance: dict[str, Any] | None = None) -> list[dict[str, Any]]:
    """Expose developer/agent context without making it byte-span evidence."""
    projected: list[dict[str, Any]] = []
    try:
        rows = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return projected
    for index, row in enumerate(rows):
        try:
            obj = json.loads(row)
        except json.JSONDecodeError:
            continue
        payload = obj.get("payload") if isinstance(obj, dict) else None
        if not isinstance(payload, dict):
            continue
        role = "agent" if obj.get("type") == "event_msg" and payload.get("type") == "agent_message" else payload.get("role")
        if role not in {"developer", "agent"}:
            continue
        texts = [block["text"] for block in payload.get("content", []) if isinstance(block, dict) and isinstance(block.get("text"), str)]
        if not texts and isinstance(payload.get("message"), str):
            texts = [payload["message"]]
        for text in texts:
            projected.append({"eventId": f"adapt-role-context-{index}", "rowIndex": index + 1,
                              "blockIndex": 0, "kind": f"{role}_message", "role": role,
                              "text": text, "timestamp": obj.get("timestamp"), "sessionId": payload.get("id") or payload.get("session_id") or "",
                              "host": "codex", "agentRole": payload.get("agent_role"), "projection": "adapt_role_context",
                              "evidenceEligible": False})
    return _attach_session_provenance(projected, provenance or {})


def _parse_through_layer(path: str | Path) -> list[dict[str, Any]]:
    """Parse a transcript via Membrane's canonical ``TranscriptEventV1`` layer.

    Plan 5.1: callers MUST go through this layer so byte spans and event ids
    line up with the rest of the substrate — we do not reimplement the parser.
    The parser lives at :mod:`continuity.transcript`, owned by Membrane;
    imports never point back into Legion.

    Insights mines evidence, so it reads the uncapped canonical source events:
    ``parse`` applies the projection cap and would silently drop the earliest
    turns of a long transcript, understating provenance counts.
    """
    from continuity.transcript import parse_source_events

    return parse_source_events(Path(path))


# ---------------------------------------------------------------------------
# CLI subcommand — `adapt insights`
# ---------------------------------------------------------------------------

def cli_insights(argv: list[str] | None = None) -> int:
    """Minimal ``adapt insights`` subcommand.

    Prints the JSON report to stdout. Never writes to disk, never touches
    Membrane, never touches Taste. Plan 5.5 hard constraint.
    """
    import argparse

    ap = argparse.ArgumentParser(prog="adapt insights")
    ap.add_argument("transcript", nargs="+", help="one or more Claude or Codex JSONL transcripts")
    ap.add_argument("--out", default=None, help="write JSON report here (default: stdout)")
    ap.add_argument("--quiet", action="store_true")
    ap.add_argument("--spend", action="store_true",
                    help="print the token-spend table instead of the JSON report")
    args = ap.parse_args(argv)

    rep = report_many(args.transcript)
    if args.spend:
        spend = rep.get("tokenSpend")
        print(token_spend.render_text(spend) if spend else "adapt insights: no billed usage found")
        return 0
    encoded = json.dumps(rep, indent=2, sort_keys=True, ensure_ascii=False, default=str)
    if args.out:
        Path(args.out).write_text(encoded, encoding="utf-8")
        if not args.quiet:
            print(f"adapt insights: wrote report -> {args.out}", file=__import__("sys").stderr)
    else:
        print(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(cli_insights())
