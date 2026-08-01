import { createHash } from "node:crypto";

const policyDigest = `sha256:${createHash("sha256").update("membrane-feedback-policy-v1:ranking-offline-only").digest("hex")}`;

export function feedbackEvent({ eventId, receiptId, outcome, recordedAt = new Date().toISOString() }) {
  if (typeof eventId !== "string" || !eventId || typeof receiptId !== "string" || !receiptId) throw new Error("feedback identity is required");
  const transition = outcome === "contradicted" ? "invalidated" : outcome;
  if (!["used", "ignored", "invalidated"].includes(transition)) throw new Error("feedback outcome is invalid");
  return {
    schema: "orthic.feedback-event.v1",
    event_id: `feedback-event-${createHash("sha256").update(`${eventId}:${receiptId}:${transition}`).digest("hex").slice(0, 24)}`,
    trace_id: eventId,
    candidate_id: receiptId,
    transition,
    recorded_at: recordedAt,
    source_event_id: eventId,
    policy_digest: policyDigest,
  };
}

export function feedbackPolicy(event) {
  return {
    delivered_proven: false,
    used_proven: event.transition === "used",
    helpful_proven: false,
    correction_required: event.transition === "invalidated",
    ranking_effect: "offline_only",
    eligibility_effect: event.transition === "invalidated" ? "retire_candidate" : "preserve_candidate",
  };
}
