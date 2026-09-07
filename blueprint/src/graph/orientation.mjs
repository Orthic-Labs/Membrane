import { buildContractRegistry } from "./contract-registry.mjs";
import { detectProjectConventions } from "./conventions.mjs";
import { buildEntryPointRegistry } from "./entry-points.mjs";
import { projectSymbolSignatures } from "./signature-projection.mjs";

export function buildColdStartOrientation(generation, files = [], { signatureLimit = 40, entryPointLimit = 24, contractLimit = 40 } = {}) {
  const directories = new Map();
  for (const file of files ?? []) {
    const path = String(file.path ?? "").replaceAll("\\", "/");
    if (!path) continue;
    const dir = path.includes("/") ? path.split("/")[0] : ".";
    directories.set(dir, (directories.get(dir) ?? 0) + 1);
  }
  const entryPoints = buildEntryPointRegistry(generation, { includeStructuralCandidates: false })
    .slice(0, Math.max(1, entryPointLimit))
    .map((entry) => ({ id: entry.node.id, kind: entry.kind, confidence: entry.confidence, path: entry.node.path ?? null, name: entry.node.qualifiedName ?? entry.node.name ?? null, evidence: entry.evidence }));
  const contracts = buildContractRegistry(generation).contracts.slice(0, Math.max(1, contractLimit));
  const signatures = projectSymbolSignatures(generation, { limit: signatureLimit });
  const conventions = detectProjectConventions(files);
  return Object.freeze({
    schemaVersion: 1,
    kind: "cold-start-orientation",
    generationId: generation?.manifest?.generationId ?? null,
    repository: {
      fileCount: files.length,
      topLevelAreas: [...directories.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([name, fileCount]) => ({ name, fileCount })).slice(0, 20),
    },
    entryPoints,
    contracts,
    signatures: signatures.signatures,
    conventions: conventions.evidence,
    omissions: [
      ...(signatures.truncated ? [{ reason: "signature_limit", limit: signatureLimit }] : []),
      ...(buildEntryPointRegistry(generation, { includeStructuralCandidates: false }).length > entryPointLimit ? [{ reason: "entrypoint_limit", limit: entryPointLimit }] : []),
      ...(buildContractRegistry(generation).contracts.length > contractLimit ? [{ reason: "contract_limit", limit: contractLimit }] : []),
    ],
  });
}
