import { createHash } from "node:crypto";

function idFor(value) {
  return `checkpoint/${createHash("sha256").update(JSON.stringify(value)).digest("hex").slice(0, 24)}`;
}

function unavailable(operation, reason = "membrane_service_unavailable") {
  return Object.freeze({ schema: "membrane.continuity-result.v1", state: "degraded", operation, reason, checkpoint: null, receipt: { verified: false, operation } });
}

function hostReference(input) {
  const reference = input?.transcriptRef ?? input?.transcriptReference;
  if (!reference || typeof reference !== "object" || typeof reference.id !== "string" || !reference.id) return null;
  return { id: reference.id, digest: typeof reference.digest === "string" ? reference.digest : null, host: typeof reference.host === "string" ? reference.host : null };
}

/**
 * Semantic continuity is Membrane-owned; raw transcript bytes remain with
 * Claude/Codex. `service` is the current authenticated Membrane operation
 * seam, so no Python/JS subprocess or local fallback is permitted.
 */
export function createContinuityClient({ service, now = () => new Date().toISOString() } = {}) {
  const call = async (operation, payload) => {
    if (typeof service !== "function") return unavailable(operation);
    try {
      const result = await service(operation, payload);
      if (!result || result.ok !== true || !result.checkpoint) return unavailable(operation, result?.reason || "continuity_unavailable");
      return Object.freeze({ schema: "membrane.continuity-result.v1", state: "available", operation, reason: "continuity_persisted", checkpoint: result.checkpoint, receipt: { verified: true, operation } });
    } catch { return unavailable(operation); }
  };
  return Object.freeze({
    async checkpoint(input = {}) {
      const reference = hostReference(input);
      if (!reference) return unavailable("checkpoint_save", "transcript_reference_required");
      const checkpoint = { schemaVersion: 1, id: input.id || idFor({ sessionId: input.sessionId || null, reference }), sessionId: input.sessionId || null, taskId: input.taskId || null, authority: input.authority || null, transcriptRef: reference, trigger: input.trigger || "unknown", createdAt: now() };
      return call("membrane_checkpoint_save", { repository: input.repository || null, caller: input.caller || null, checkpoint });
    },
    async restore(input = {}) {
      if (typeof input.id !== "string" || !input.id) return unavailable("checkpoint_load", "checkpoint_id_required");
      return call("membrane_checkpoint_load", { repository: input.repository || null, caller: input.caller || null, id: input.id, asOfMs: input.asOfMs });
    },
  });
}
