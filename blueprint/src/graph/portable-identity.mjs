import { createHash } from "node:crypto";

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
}

function digest(prefix, value) {
  return `${prefix}:sha256:${createHash("sha256").update(JSON.stringify(stable(value))).digest("hex")}`;
}

export function portableSemanticIdentity(fact, context = {}) {
  if (!fact || typeof fact !== "object") return null;
  if (typeof fact.portableId === "string" && fact.portableId.startsWith("bp:")) return fact.portableId;

  // SCIP symbols are designed as cross-file semantic identities. Keep the raw
  // symbol as evidence but expose a compact deterministic interchange ID.
  if (typeof fact.symbol === "string" && fact.symbol.trim()) {
    return digest("bp:scip", { symbol: fact.symbol.trim() });
  }

  // Canonical framework/domain identities are exact only when their provider
  // has already resolved the domain key (route/topic/table/service/etc.).
  if (fact.domainIdentity && typeof fact.domainIdentity === "object") {
    return digest("bp:domain", fact.domainIdentity);
  }

  // Package/language/descriptor identities may be emitted by compiler adapters
  // that do not use SCIP. All fields are required: missing scope means no ID.
  const packageIdentity = fact.packageIdentity ?? context.packageIdentity ?? null;
  const language = fact.language ?? context.language ?? null;
  const signature = fact.semanticSignature ?? fact.signatureIdentity ?? null;
  if (packageIdentity && language && signature) {
    return digest("bp:symbol", { packageIdentity, language, signature });
  }
  return null;
}

export function attachPortableIdentities(generation, context = {}) {
  let attached = 0;
  for (const node of generation?.nodes ?? []) {
    const portableId = portableSemanticIdentity(node, context);
    if (!portableId || node.portableId === portableId) continue;
    node.portableId = portableId;
    attached += 1;
  }
  for (const edge of generation?.edges ?? []) {
    if (edge.portableId) continue;
    const source = generation.nodes?.find((node) => node.id === edge.source)?.portableId ?? null;
    const target = generation.nodes?.find((node) => node.id === edge.target)?.portableId ?? null;
    if (!source || !target) continue;
    edge.portableId = digest("bp:relation", { kind: edge.kind, source, target });
    attached += 1;
  }
  return { schemaVersion: 1, attached };
}

export function portableIdentityKey(value) {
  return typeof value === "string" && /^bp:(?:scip|domain|symbol|relation):sha256:[0-9a-f]{64}$/.test(value) ? value : null;
}
