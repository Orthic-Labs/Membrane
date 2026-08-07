// MBR-502: passive Git/read provenance adapter — JS twin.
//
// Mirrors the Rust `ProvenanceRowV1` shape so the runtime can persist a
// row produced on the JS side without any conversion. This module is
// strictly passive: it does NOT read file bodies, it does NOT resolve
// paths outside the workspace, and it does NOT open a network socket.
// The runtime is responsible for calling `observe()` here with the
// already-captured working tree snapshot — the adapter only formats.

export const PROVENANCE_KIND = "git_read";
export const PROVENANCE_SCHEMA_VERSION = 1;

/**
 * Build a provenance row from JS-shaped inputs. Field names use
 * `camelCase` to match the Rust serde contract; denormalizing here
 * keeps the wire format identical to the Rust writer.
 *
 * @param {object} input
 * @param {string} input.operation        Short verb for what the runtime did.
 * @param {string|null} [input.clientId] Client identifier, when known.
 * @param {string|null} [input.scopeGrant] Scope-grant token or digest.
 * @param {object} input.workingTree     Already-captured working tree snapshot.
 * @param {string} input.installationId  Stable per-installation id.
 * @param {number} input.recordedAtUnixMs Wall-clock wall time.
 * @returns {object} The provenance row ready for JSONL serialization.
 */
export function provenanceRowFromJs({
  operation,
  clientId = null,
  scopeGrant = null,
  workingTree,
  installationId,
  recordedAtUnixMs,
}) {
  if (typeof operation !== "string" || operation.length === 0) {
    throw new TypeError("provenanceRowFromJs: operation must be a non-empty string");
  }
  if (typeof installationId !== "string" || installationId.length === 0) {
    throw new TypeError("provenanceRowFromJs: installationId must be a non-empty string");
  }
  if (typeof recordedAtUnixMs !== "number" || !Number.isFinite(recordedAtUnixMs)) {
    throw new TypeError("provenanceRowFromJs: recordedAtUnixMs must be a finite number");
  }
  if (workingTree === null || typeof workingTree !== "object") {
    throw new TypeError("provenanceRowFromJs: workingTree must be an object");
  }
  return {
    schemaVersion: PROVENANCE_SCHEMA_VERSION,
    installationId,
    operation,
    clientId,
    scopeGrant,
    workingTree,
    recordedAtUnixMs,
  };
}
