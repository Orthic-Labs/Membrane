"""Hash-bound Membrane event to durable Morph preference learning."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Callable

import morph_persistence
import admission
import observable_events
import preference_record
import rule_key


LEARNABLE_EVENT_TYPES = frozenset({"user_correction", "user_preference", "user_instruction"})


class MorphLearningError(RuntimeError):
    """Raised when event provenance, admission, persistence, or delivery fails."""


def _sha256_text(value: str) -> str:
    return f"sha256:{hashlib.sha256(value.encode('utf-8')).hexdigest()}"


def _canonical_sha256(value: object) -> str:
    encoded = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode("utf-8")
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


@dataclass(frozen=True)
class AdmittedLearning:
    event_id: str
    trace_id: str
    source_id: str
    evidence_sha256: str
    rule_key: str
    record: preference_record.PreferenceRecord
    admission_reason: str = "ok"
    approval_event_id: str = ""
    feedback_sha256: str = ""

    @property
    def digest(self) -> str:
        return _canonical_sha256({
            "event_id": self.event_id,
            "trace_id": self.trace_id,
            "source_id": self.source_id,
            "evidence_sha256": self.evidence_sha256,
            "rule_key": self.rule_key,
            "record": self.record.to_dict(),
            "admission_reason": self.admission_reason,
            "approval_event_id": self.approval_event_id,
            "feedback_sha256": self.feedback_sha256,
        })

    @property
    def approved(self) -> bool:
        return bool(self.approval_event_id) and self.record.lifecycle_state == "active"


def approval_text(learning: AdmittedLearning) -> str:
    """Exact user feedback required to promote one event-derived proposal."""
    return f"approve morph proposal {learning.digest}"


def admit_user_event(
    event: dict,
    *,
    evidence_text: str,
    scope: str,
    category: str,
    canonical_rules: rule_key.RuleIndex | dict | None = None,
) -> AdmittedLearning:
    """Create a quarantined proposal from exact user-authored event content."""
    observable_events._validate(event)
    if event["origin"] != "user":
        raise MorphLearningError(f"origin-not-user:{event['origin']}")
    if event["event_type"] not in LEARNABLE_EVENT_TYPES:
        raise MorphLearningError(f"event-not-learnable:{event['event_type']}")
    evidence_sha256 = _sha256_text(evidence_text)
    if event["content_ref_or_digest"] != evidence_sha256:
        raise MorphLearningError("event-content-digest-mismatch")
    normalized_category = admission.normalize_category(category)
    record_id = preference_record.derive_id(scope, normalized_category, evidence_text)
    target = {
        "action": "add",
        "name": record_id,
        "scope": scope,
        "category": normalized_category,
        "rule": evidence_text,
        "origin": "user_turn",
        "evidence_text": evidence_text,
        "record_type": "standing_preference",
    }
    admitted, reason = admission.admit(
        "add", target, canonical_rules=canonical_rules or rule_key.RuleIndex.from_mapping({})
    )
    if not admitted:
        raise MorphLearningError(f"admission-rejected:{reason}")
    source_id = (
        f"install:{event['installation_id']}:{event['client_id']}:{event['event_id']}"
    )
    record = preference_record.PreferenceRecord.from_synthesis(
        {
            **target,
            "confidence": 1.0,
            "observations": 1,
            "needs_review": True,
            "retrieval_aliases": (evidence_text,),
        },
        scope=scope,
        source_ids=(source_id,),
        lifecycle_state="candidate",
    )
    key = rule_key.RuleKey(scope=scope, record_id=record.id)
    return AdmittedLearning(
        event_id=event["event_id"],
        trace_id=event["trace_id"],
        source_id=source_id,
        evidence_sha256=evidence_sha256,
        rule_key=key.formatted(),
        record=record,
    )


def approve_learning(
    learning: AdmittedLearning,
    *,
    feedback_event: dict,
    feedback_text: str,
) -> AdmittedLearning:
    """Bind separate user feedback before an event proposal can enter recall."""
    if learning.approved:
        raise MorphLearningError("learning-already-approved")
    observable_events._validate(feedback_event)
    if feedback_event["origin"] != "user":
        raise MorphLearningError(f"feedback-origin-not-user:{feedback_event['origin']}")
    if feedback_event["event_type"] not in LEARNABLE_EVENT_TYPES:
        raise MorphLearningError(f"feedback-event-not-learnable:{feedback_event['event_type']}")
    installation_id = learning.source_id.split(":", 3)[1]
    if (
        feedback_event["installation_id"] != installation_id
        or feedback_event["trace_id"] != learning.trace_id
    ):
        raise MorphLearningError("feedback-lineage-mismatch")
    if feedback_text != approval_text(learning):
        raise MorphLearningError("feedback-approval-text-mismatch")
    if feedback_event["content_ref_or_digest"] != _sha256_text(feedback_text):
        raise MorphLearningError("feedback-content-digest-mismatch")
    feedback_source = (
        f"install:{feedback_event['installation_id']}:"
        f"{feedback_event['client_id']}:{feedback_event['event_id']}"
    )
    record = replace(
        preference_record.transition_lifecycle(learning.record, "active"),
        needs_review=False,
        source_ids=tuple((*learning.record.source_ids, feedback_source)),
    )
    return replace(
        learning,
        record=record,
        approval_event_id=feedback_event["event_id"],
        feedback_sha256=_sha256_text(feedback_text),
    )


def persist_learning(
    learning: AdmittedLearning,
    *,
    token_file: Path,
    base_url: str,
    installation_id: str,
) -> dict:
    """Persist only an explicitly approved event-learning proposal."""
    if not learning.approved:
        raise MorphLearningError("proposal-not-approved")
    receipt = morph_persistence.persist_manifest_batch(
        (learning.record,),
        manifest_batch_id=f"morph-event-{learning.event_id}-{learning.digest}",
        installation_id=installation_id,
        token_file=token_file,
        base_url=base_url,
    )
    if receipt.get("complete") is not True:
        raise MorphLearningError("persistence-incomplete")
    return {
        **receipt,
        "learning": {
            "event_id": learning.event_id,
            "trace_id": learning.trace_id,
            "rule_key": learning.rule_key,
            "approval_event_id": learning.approval_event_id,
            "feedback_sha256": learning.feedback_sha256,
        },
    }


def verify_next_use(
    learning: AdmittedLearning,
    *,
    get_memory: Callable[[str], str],
    recall: Callable[[str, str], str],
) -> dict:
    """Use independent read paths to prove approved storage plus next-task delivery."""
    if not learning.approved:
        raise MorphLearningError("proposal-not-approved")
    body = get_memory(learning.rule_key)
    if learning.record.rule not in body:
        raise MorphLearningError("independent-readback-mismatch")
    delivered = recall(learning.record.rule, learning.record.scope)
    if learning.rule_key not in delivered and learning.record.id not in delivered:
        raise MorphLearningError("next-use-delivery-missing")
    return {
        "event_id": learning.event_id,
        "trace_id": learning.trace_id,
        "candidate_digest": learning.digest,
        "rule_key": learning.rule_key,
        "approval_event_id": learning.approval_event_id,
        "readback_sha256": _sha256_text(body),
        "delivery_sha256": _sha256_text(delivered),
        "delivered": True,
    }
