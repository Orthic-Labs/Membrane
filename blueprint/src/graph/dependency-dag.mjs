import { createHash } from "node:crypto";

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
}
function hash(value) { return `sha256:${createHash("sha256").update(JSON.stringify(stable(value))).digest("hex")}`; }

export const PROJECTION_DEPENDENCIES = Object.freeze({
  bm25: Object.freeze(["source", "provider", "schema", "generation"]),
  structural_search: Object.freeze(["source", "provider", "schema", "generation"]),
  signatures: Object.freeze(["source", "provider", "schema", "generation"]),
  contracts: Object.freeze(["source", "provider", "config", "schema", "generation"]),
  processes: Object.freeze(["source", "provider", "config", "schema", "generation"]),
  conventions: Object.freeze(["source", "config", "generation"]),
  orientation: Object.freeze(["source", "provider", "config", "schema", "generation"]),
});

export function buildProjectionDependencyDag({
  sourceHash = null,
  providerDigest = null,
  configDigest = null,
  schemaVersion = null,
  generationId = null,
} = {}) {
  const parents = Object.freeze({
    source: sourceHash,
    provider: providerDigest,
    config: configDigest,
    schema: schemaVersion,
    generation: generationId,
  });
  const nodes = [
    ...Object.keys(parents).map((id) => ({ id: `parent:${id}`, kind: "parent", value: parents[id] })),
    ...Object.keys(PROJECTION_DEPENDENCIES).map((id) => ({ id: `projection:${id}`, kind: "projection" })),
  ];
  const edges = [];
  for (const [projection, dependencies] of Object.entries(PROJECTION_DEPENDENCIES)) {
    for (const dependency of dependencies) edges.push({ from: `parent:${dependency}`, to: `projection:${projection}` });
  }
  return Object.freeze({ schemaVersion: 1, parents, nodes: Object.freeze(nodes), edges: Object.freeze(edges) });
}

export function invalidatedProjections(dag, changedParents = []) {
  const changed = new Set(changedParents.map((value) => String(value).replace(/^parent:/, "")));
  const projections = [];
  for (const [projection, dependencies] of Object.entries(PROJECTION_DEPENDENCIES)) {
    if (dependencies.some((dependency) => changed.has(dependency))) projections.push(projection);
  }
  return projections.sort();
}

export function projectionFingerprint(dag, projection) {
  const dependencies = PROJECTION_DEPENDENCIES[projection];
  if (!dependencies) throw Object.assign(new Error(`unknown projection ${projection}`), { code: "projection_unknown" });
  return hash({ projection, parents: Object.fromEntries(dependencies.map((dependency) => [dependency, dag.parents[dependency] ?? null])) });
}

export class ProjectionCache {
  constructor({ maxEntries = 16 } = {}) {
    this.maxEntries = Math.max(1, Number(maxEntries) || 16);
    this.entries = new Map();
  }

  getOrBuild(projection, dag, builder) {
    const fingerprint = projectionFingerprint(dag, projection);
    const existing = this.entries.get(projection);
    if (existing?.fingerprint === fingerprint) return { value: existing.value, cache: "hit", fingerprint };
    const value = builder();
    this.entries.set(projection, { fingerprint, value });
    while (this.entries.size > this.maxEntries) this.entries.delete(this.entries.keys().next().value);
    return { value, cache: existing ? "invalidated" : "miss", fingerprint };
  }

  invalidate(changedParents, dag) {
    const projections = invalidatedProjections(dag, changedParents);
    for (const projection of projections) this.entries.delete(projection);
    return projections;
  }
}
