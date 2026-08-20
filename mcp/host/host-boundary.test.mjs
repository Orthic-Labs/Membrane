import assert from "node:assert/strict";
import test from "node:test";
import { CAPABILITY_MATRIX_DIGEST, capabilityFor, validateCapabilityMatrix } from "./capability-matrix.mjs";
import { interceptEvidence, isHonestDelivery } from "./evidence-interceptor.mjs";
import { createContinuityClient } from "./continuity.mjs";
import { consumeCandidateSet } from "./candidate-set.mjs";

test("host capability matrix covers only observed Claude/Codex seams", () => {
  assert.equal(validateCapabilityMatrix().valid, true);
  assert.match(CAPABILITY_MATRIX_DIGEST, /^sha256:[0-9a-f]{64}$/u);
  assert.equal(capabilityFor("claude_code", "delegated_agent_egress"), "projection");
  assert.equal(capabilityFor("codex", "delegated_agent_egress"), "unavailable");
  assert.equal(capabilityFor("unknown", "UserPromptSubmit"), "unavailable");
});

test("evidence interception never transports Legion directive and proves delivery", async () => {
  const degraded = await interceptEvidence({ directive: "change the plan", evidence: { traceId: "t1" } }, { reduce: async () => ({ traceId: "t1", blocks: [] }) });
  assert.equal(degraded.state, "degraded");
  assert.equal(degraded.reason, "delivery_unverified");
  const delivered = await interceptEvidence({ directive: "change the plan", evidence: { traceId: "t1" } }, {
    reduce: async (evidence) => ({ traceId: evidence.traceId, blocks: [{ id: "evidence", text: "data" }] }),
    verifyDelivery: async (_packet, receipt) => receipt.traceId === "t1",
  });
  assert.equal(delivered.state, "context_enforced");
  assert.equal(delivered.directive, null);
  assert.equal(delivered.receipt.verified, true);
  assert.equal(isHonestDelivery(delivered), true);
});

test("continuity sends host transcript references through current service", async () => {
  const calls = [];
  const client = createContinuityClient({ service: async (operation, payload) => {
    calls.push({ operation, payload });
    return { ok: true, checkpoint: payload.checkpoint || { id: payload.id, restored: true } };
  }, now: () => "2026-08-20T00:00:00.000Z" });
  const saved = await client.checkpoint({ sessionId: "s1", transcriptRef: { id: "host-log-1", digest: "sha256:x", host: "codex" }, trigger: "PreCompact" });
  assert.equal(saved.state, "available");
  assert.equal(calls[0].operation, "membrane_checkpoint_save");
  assert.equal(calls[0].payload.checkpoint.transcriptRef.id, "host-log-1");
  assert.equal("bytes" in calls[0].payload.checkpoint.transcriptRef, false);
  const restored = await client.restore({ id: saved.checkpoint.id });
  assert.equal(restored.state, "available");
  assert.equal(calls[1].operation, "membrane_checkpoint_load");
});

test("continuity reports typed degradation instead of fallback when service is absent", async () => {
  const client = createContinuityClient();
  const result = await client.checkpoint({ transcriptRef: { id: "host-log-1" } });
  assert.equal(result.state, "degraded");
  assert.equal(result.reason, "membrane_service_unavailable");
});

test("candidate consumer requires Blueprint trace and index identity", () => {
  const consumed = consumeCandidateSet({ traceId: "t1", freshness: { indexedAt: "2026-08-20T00:00:00.000Z" }, candidates: [], omissions: [] });
  assert.equal(consumed.state, "available");
  assert.equal(consumed.candidateSet.indexedAt, "2026-08-20T00:00:00.000Z");
  assert.match(consumed.receipt.candidateSetDigest, /^sha256:[0-9a-f]{64}$/u);
  assert.equal(consumeCandidateSet({ traceId: "t1", freshness: {} }).state, "degraded");
});
