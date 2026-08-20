import { createHash } from "node:crypto";

const SCHEMA = "membrane.context-candidate-set.v1";

function digest(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

/**
 * Normalize a Blueprint candidate envelope at Membrane's consumer boundary.
 * `indexedAt` is copied only when Blueprint supplied it; no host timestamp is
 * invented for an incomplete graph receipt.
 */
export function consumeCandidateSet(value, { traceId = null } = {}) {
  if (!value || typeof value !== "object") return { state: "degraded", reason: "candidate_set_missing", candidateSet: null };
  const resolvedTrace = typeof value.traceId === "string" && value.traceId ? value.traceId : traceId;
  const indexedAt = value.indexedAt || value.freshness?.indexedAt;
  if (typeof resolvedTrace !== "string" || !resolvedTrace || typeof indexedAt !== "string" || !indexedAt) return { state: "degraded", reason: "candidate_set_identity_incomplete", candidateSet: null };
  const candidateSet = Object.freeze({ ...value, schema: SCHEMA, traceId: resolvedTrace, indexedAt });
  return { state: "available", reason: "candidate_set_consumed", candidateSet, receipt: { schema: "membrane.candidate-set-receipt.v1", traceId: resolvedTrace, indexedAt, candidateSetDigest: digest(candidateSet) } };
}
