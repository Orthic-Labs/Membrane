import { createHash } from "node:crypto";
import { capabilityFor } from "./capability-matrix.mjs";

const STATES = Object.freeze(["context_enforced", "degraded"]);

function digest(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function degraded(reason, detail = {}) {
  return Object.freeze({ schema: "membrane.host-evidence-delivery.v1", state: "degraded", reason, directive: null, evidence: null, receipt: { verified: false, reason, ...detail } });
}

/**
 * Intercept only Membrane evidence at an observed host egress seam. Directive
 * bytes remain Legion-owned and are deliberately discarded from this result.
 * Reduction is injected from Push; this adapter never selects or truncates.
 */
export async function interceptEvidence(input = {}, { client = "claude_code", event = "tool_result_egress", reduce, verifyDelivery } = {}) {
  const evidence = input.evidence;
  if (!evidence || typeof evidence !== "object") return degraded("evidence_unavailable");
  if (typeof reduce !== "function") return degraded("push_reducer_unavailable");
  const level = capabilityFor(client, event);
  if (level === "unavailable") return degraded("host_seam_unavailable", { client, event });
  let packet;
  try { packet = await reduce(evidence, { client, event, level }); } catch { return degraded("push_reduction_failed", { client, event }); }
  if (!packet || typeof packet !== "object") return degraded("push_packet_unavailable", { client, event });
  const receipt = { schema: "membrane.context-receipt.v1", traceId: packet.traceId || evidence.traceId || null, packetDigest: digest(packet), verified: false, capability: level };
  if (typeof verifyDelivery !== "function") return degraded("delivery_unverified", { client, event });
  let verified = false;
  try { verified = (await verifyDelivery(packet, receipt)) === true; } catch { verified = false; }
  if (!verified) return degraded("delivery_unverified", { client, event });
  return Object.freeze({ schema: "membrane.host-evidence-delivery.v1", state: "context_enforced", reason: "verified_delivery", directive: null, evidence: packet, receipt: Object.freeze({ ...receipt, verified: true }) });
}

export function isHonestDelivery(result) {
  return Boolean(result && STATES.includes(result.state) && (result.state !== "context_enforced" || result.receipt?.verified === true));
}
