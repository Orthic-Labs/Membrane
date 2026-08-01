import assert from "node:assert/strict";
import test from "node:test";
import { feedbackEvent, feedbackPolicy } from "./feedback-loop.mjs";

test("feedback event remains conservative and idempotent", () => {
  const args = { eventId: "event-1", receiptId: "candidate-1", outcome: "used", recordedAt: "2026-08-01T00:00:00.000Z" };
  const first = feedbackEvent(args);
  assert.deepEqual(feedbackEvent(args), first);
  assert.equal(feedbackPolicy(first).delivered_proven, false);
  assert.equal(feedbackPolicy(first).helpful_proven, false);
  assert.equal(feedbackPolicy(first).ranking_effect, "offline_only");
});

test("contradiction invalidates eligibility without silently changing rank", () => {
  const event = feedbackEvent({ eventId: "event-2", receiptId: "candidate-2", outcome: "contradicted", recordedAt: "2026-08-01T00:00:00.000Z" });
  assert.equal(event.transition, "invalidated");
  assert.equal(feedbackPolicy(event).eligibility_effect, "retire_candidate");
  assert.equal(feedbackPolicy(event).ranking_effect, "offline_only");
});
