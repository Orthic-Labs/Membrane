// Forge-consumable orientation evidence — typed blob derived from receipts.
//
// Blueprint emits evidence; Forge attests and gates signoff. This module
// does not enforce signoff — it only shapes the record Forge can consume.

import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync, renameSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

export const ORIENTATION_EVIDENCE_KIND = "blueprint_orientation";
export const ORIENTATION_EVIDENCE_SCHEMA_VERSION = 1;

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map((item) => stableStringify(item)).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}

export function sha256Hex(value) {
  return createHash("sha256").update(typeof value === "string" ? value : stableStringify(value)).digest("hex");
}

/**
 * Digest a ContextCandidateSet (or equivalent) for receipt / evidence binding.
 */
export function candidateSetDigest(candidateSet) {
  const payload = {
    schemaVersion: candidateSet?.schemaVersion ?? 1,
    provider: candidateSet?.provider ?? null,
    freshness: candidateSet?.freshness ?? null,
    candidates: (candidateSet?.candidates ?? []).map((c) => ({
      id: c.id,
      sourceRef: c.sourceRef,
      sourceHash: c.sourceHash,
      protected: c.protected,
      exact: c.exact,
    })),
    omissions: (candidateSet?.omissions ?? []).map((o) => ({ id: o.id, reason: o.reason })),
  };
  return `sha256:${sha256Hex(payload)}`;
}

/**
 * Build a Forge-compatible evidence record from an orientation receipt.
 *
 * @param {object} receipt
 * @param {{ supportsOrRefutes?: "supports"|"refutes" }} [options]
 */
export function buildOrientationEvidence(receipt, options = {}) {
  if (!receipt?.receiptId) {
    throw new Error("orientation evidence requires a receipt with receiptId");
  }
  const overlayRevision = Number(receipt.overlayRevision ?? 0);
  const manifestDigest = receipt.manifestDigest ?? "unknown";
  const contentHash = receipt.candidateSetDigest
    ?? (receipt.contentHash ? String(receipt.contentHash) : `sha256:${sha256Hex(receipt.receiptId)}`);
  return {
    schemaVersion: ORIENTATION_EVIDENCE_SCHEMA_VERSION,
    kind: ORIENTATION_EVIDENCE_KIND,
    locator: `blueprint://receipt/${receipt.receiptId}`,
    content_hash: contentHash,
    invalidation_key: `${manifestDigest}:${overlayRevision}`,
    trust_class: "host_attested",
    supports_or_refutes: options.supportsOrRefutes ?? "supports",
    receiptId: receipt.receiptId,
    generationId: receipt.generationId ?? null,
    manifestDigest: receipt.manifestDigest ?? null,
    sessionId: receipt.sessionId ?? null,
    taskId: receipt.taskId ?? null,
    repoIdentity: receipt.repoIdentity ?? null,
    issuedAt: receipt.issuedAt ?? null,
    status: receipt.status ?? "active",
  };
}

/**
 * Write evidence to a file Forge can attest (atomic replace).
 * Returns the absolute path written.
 */
export function writeOrientationEvidenceFile(receipt, outPath, options = {}) {
  const evidence = buildOrientationEvidence(receipt, options);
  const target = resolve(outPath);
  mkdirSync(dirname(target), { recursive: true });
  const tmp = `${target}.${process.pid}.${Date.now()}.tmp`;
  writeFileSync(tmp, `${JSON.stringify(evidence, null, 2)}\n`, "utf8");
  renameSync(tmp, target);
  return { path: target, evidence };
}

/**
 * Default evidence filename for a receipt under a directory.
 */
export function defaultEvidencePath(dir, receiptId) {
  return join(resolve(dir), `blueprint-orientation-${receiptId}.json`);
}
