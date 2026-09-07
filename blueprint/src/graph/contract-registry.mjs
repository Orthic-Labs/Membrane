import { createHash } from "node:crypto";

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
}
function digest(value) { return `sha256:${createHash("sha256").update(JSON.stringify(stable(value))).digest("hex")}`; }

function rolesForNode(generation, node) {
  const roles = new Set(node.contractRoles ?? (node.contractRole ? [node.contractRole] : []));
  for (const edge of generation.edges ?? []) {
    if (edge.target !== node.id) continue;
    if (edge.kind === "PRODUCES") roles.add("provider");
    if (edge.kind === "CONSUMES" || edge.kind === "USES") roles.add("consumer");
    if (edge.kind === "ROUTES_TO") roles.add("provider");
  }
  return [...roles].sort();
}

function contractFromNode(generation, node, repoId) {
  const labels = new Set(node.labels ?? []);
  let kind = null;
  let address = null;
  if (labels.has("HttpRoute")) {
    kind = "http";
    address = `${String(node.method ?? "ANY").toUpperCase()} ${node.routePath ?? node.name}`;
  } else if (labels.has("EventTopic")) {
    kind = "event";
    address = node.domainIdentity?.address ?? node.name;
  } else if (labels.has("ToolContract")) {
    kind = "tool";
    address = node.domainIdentity?.address ?? node.name;
  } else if (labels.has("UiRoute")) {
    kind = "ui_route";
    address = node.domainIdentity?.address ?? node.name;
  }
  if (!kind || !address) return null;
  const schema = node.contractSchema ?? null;
  const contractKey = digest({ kind, address, schema });
  return {
    contractId: `contract:${kind}:${contractKey.replace(/^sha256:/, "")}`,
    contractKey,
    repoId,
    kind,
    address,
    schema,
    roles: rolesForNode(generation, node),
    nodeId: node.id,
    portableId: node.portableId ?? null,
    evidence: node.evidence ?? [],
  };
}

export function buildContractRegistry(generation, { repoId = null } = {}) {
  const contracts = [];
  for (const node of generation?.nodes ?? []) {
    const contract = contractFromNode(generation, node, repoId);
    if (contract) contracts.push(contract);
  }
  contracts.sort((left, right) => left.kind.localeCompare(right.kind) || left.address.localeCompare(right.address) || String(left.repoId).localeCompare(String(right.repoId)));
  return Object.freeze({ schemaVersion: 1, repoId, generationId: generation?.manifest?.generationId ?? null, contracts: Object.freeze(contracts) });
}

/** Exact contract bridges only. Names that merely resemble one another never join. */
export function bridgeContractRegistries(registries = []) {
  const byKey = new Map();
  for (const registry of registries ?? []) {
    for (const contract of registry?.contracts ?? []) {
      if (!byKey.has(contract.contractKey)) byKey.set(contract.contractKey, []);
      byKey.get(contract.contractKey).push(contract);
    }
  }
  const bridges = [];
  for (const [contractKey, contracts] of byKey) {
    const providers = contracts.filter((contract) => contract.roles.includes("provider"));
    const consumers = contracts.filter((contract) => contract.roles.includes("consumer"));
    for (const consumer of consumers) for (const provider of providers) {
      if (!consumer.repoId || !provider.repoId || consumer.repoId === provider.repoId) continue;
      bridges.push({
        id: `bridge:${digest({ contractKey, consumer: consumer.repoId, provider: provider.repoId }).replace(/^sha256:/, "")}`,
        contractKey,
        kind: consumer.kind,
        address: consumer.address,
        consumer: { repoId: consumer.repoId, nodeId: consumer.nodeId, generationId: registries.find((r) => r.repoId === consumer.repoId)?.generationId ?? null },
        provider: { repoId: provider.repoId, nodeId: provider.nodeId, generationId: registries.find((r) => r.repoId === provider.repoId)?.generationId ?? null },
        evidence: [...(consumer.evidence ?? []), ...(provider.evidence ?? [])],
      });
    }
  }
  bridges.sort((a, b) => a.id.localeCompare(b.id));
  return Object.freeze({ schemaVersion: 1, bridges: Object.freeze(bridges) });
}

export function stitchContractTraces(registries = []) {
  const bridgeProjection = bridgeContractRegistries(registries);
  const traces = bridgeProjection.bridges.map((bridge) => ({
    id: `trace:${bridge.id.slice("bridge:".length)}`,
    contractKey: bridge.contractKey,
    steps: [
      { repoId: bridge.consumer.repoId, nodeId: bridge.consumer.nodeId, role: "consumer", generationId: bridge.consumer.generationId },
      { repoId: bridge.provider.repoId, nodeId: bridge.provider.nodeId, role: "provider", generationId: bridge.provider.generationId },
    ],
    evidence: bridge.evidence,
  }));
  return Object.freeze({ schemaVersion: 1, bridges: bridgeProjection.bridges, traces: Object.freeze(traces) });
}
