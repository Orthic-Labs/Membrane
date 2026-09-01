// D42: signed run ledger — HMAC/Sigstore-compatible chain over version,
// commit, generation, provider/rule digests, inputs, outputs, and artifact
// hashes. Missing or tampered links fail verification.

import { createHmac, createHash } from "node:crypto";

export function ledgerLink({ key, previous, version, commit, generationId, providerDigests, ruleDigests, inputs, outputs, artifactHashes }) {
  const payload = {
    previous,
    version,
    commit,
    generationId,
    providerDigests,
    ruleDigests,
    inputs,
    outputs,
    artifactHashes,
  };
  const body = JSON.stringify(payload);
  const digest = createHash("sha256").update(body).digest("hex");
  const signature = createHmac("sha256", key).update(digest).digest("hex");
  return { schemaVersion: 1, payload, digest, signature };
}

export function verifyLedgerLink(link, { key, expectedPrevious = null }) {
  if (!link || !link.digest || !link.signature) return { ok: false, reason: "missing_link_fields" };
  if (expectedPrevious !== null && link.payload?.previous !== expectedPrevious) return { ok: false, reason: "broken_chain" };
  const recomputedDigest = createHash("sha256").update(JSON.stringify(link.payload)).digest("hex");
  if (recomputedDigest !== link.digest) return { ok: false, reason: "tampered_payload" };
  const expectedSignature = createHmac("sha256", key).update(link.digest).digest("hex");
  if (expectedSignature !== link.signature) return { ok: false, reason: "bad_signature" };
  return { ok: true };
}

export function verifyLedgerChain(links = [], key) {
  let previous = null;
  for (const link of links) {
    const result = verifyLedgerLink(link, { key, expectedPrevious: previous });
    if (!result.ok) return { ok: false, brokenAt: link.payload?.version ?? "unknown", reason: result.reason };
    previous = link.digest;
  }
  return { ok: true, links: links.length };
}
