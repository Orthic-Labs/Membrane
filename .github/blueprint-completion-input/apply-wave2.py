from pathlib import Path
import subprocess

ROOT = Path.cwd()


def replace_once(path, old, new):
    p = ROOT / path
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one match in {path}, got {count}: {old[:140]!r}")
    p.write_text(text.replace(old, new, 1))


def write(path, content):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)


def tests(*names):
    subprocess.run(["node", "--test", *[f"tests/{name}" for name in names]], cwd=ROOT / "blueprint", check=True)


def commit(message, files):
    subprocess.run(["git", "add", *files], cwd=ROOT, check=True)
    subprocess.run(["git", "commit", "-m", message], cwd=ROOT, check=True)


subprocess.run(["git", "config", "user.name", "Blueprint completion automation"], cwd=ROOT, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=ROOT, check=True)

# ---------------------------------------------------------------------------
# 1. Production graph build: run the new semantic/framework layers and keep
#    incremental augmentation idempotent.
# ---------------------------------------------------------------------------
structural = "blueprint/src/graph/structural-intelligence.mjs"
replace_once(
    structural,
    '''  generation.edges.push(...edges);
  const mro = mroProjection(generation, hierarchyEdges);
''',
    '''  const existingEdgeIds = new Set(generation.edges.map((edge) => edge.id));
  const newEdges = edges.filter((edge) => !existingEdgeIds.has(edge.id));
  generation.edges.push(...newEdges);
  const mro = mroProjection(generation, hierarchyEdges);
''',
)
replace_once(
    structural,
    '''    overrideEdges: edges.filter((edge) => edge.kind === "OVERRIDES").length,
    dynamicDispatchEdges: edges.filter((edge) => edge.kind === "HANDLES").length,
''',
    '''    overrideEdges: newEdges.filter((edge) => edge.kind === "OVERRIDES").length,
    dynamicDispatchEdges: newEdges.filter((edge) => edge.kind === "HANDLES").length,
''',
)

build = "blueprint/src/providers/build.mjs"
replace_once(
    build,
    'import { auditSourceDispositions } from "./source-disposition.mjs";\n',
    'import { auditSourceDispositions } from "./source-disposition.mjs";\nimport { augmentStructuralIntelligence, STRUCTURAL_INTELLIGENCE_PROVIDER } from "../graph/structural-intelligence.mjs";\nimport { augmentFrameworkIntelligence, FRAMEWORK_INTELLIGENCE_PROVIDER } from "../graph/framework-intelligence.mjs";\nimport { attachPortableIdentities } from "../graph/portable-identity.mjs";\nimport { detectProjectConventions } from "../graph/conventions.mjs";\n',
)
replace_once(
    build,
    '''  const summaries = {
    ingestion: auditSourceDispositions(root, files),
    modules: addModuleEvidence(generation, files, root),
    frameworks: addFrameworkEvidence(generation, files),
    ...addSchemaAndIacEvidence(generation, files),
    scip: addScipEvidence(generation, files, root, options),
    bridges: addBridgeEvidence(generation, files),
  };
  const layers = [
''',
    '''  const summaries = {
    ingestion: auditSourceDispositions(root, files),
    modules: addModuleEvidence(generation, files, root),
    frameworks: addFrameworkEvidence(generation, files),
    ...addSchemaAndIacEvidence(generation, files),
    scip: addScipEvidence(generation, files, root, options),
    bridges: addBridgeEvidence(generation, files),
  };
  summaries.structuralIntelligence = augmentStructuralIntelligence(generation, files);
  summaries.frameworkIntelligence = augmentFrameworkIntelligence(generation, files);
  summaries.portableIdentity = attachPortableIdentities(generation);
  summaries.conventions = detectProjectConventions(files);
  const layers = [
''',
)
replace_once(
    build,
    '''    { id: bridgeSeamProvider.id, version: bridgeSeamProvider.version, role: "supplemental", precisionTier: "EXACT_SYNTAX" },
  ];
''',
    '''    { id: bridgeSeamProvider.id, version: bridgeSeamProvider.version, role: "supplemental", precisionTier: "EXACT_SYNTAX" },
    { id: STRUCTURAL_INTELLIGENCE_PROVIDER.id, version: STRUCTURAL_INTELLIGENCE_PROVIDER.version, role: "supplemental", precisionTier: "EXACT_OR_TYPED_UNRESOLVED" },
    { id: FRAMEWORK_INTELLIGENCE_PROVIDER.id, version: FRAMEWORK_INTELLIGENCE_PROVIDER.version, role: "supplemental", precisionTier: "EVIDENCE_BOUND" },
  ];
''',
)
replace_once(
    build,
    '''  addScipEvidence(generation, files, root, options, new Set([normalizePath(file.path)]), true);
  addBridgeEvidence(generation, selected);
  return generation;
''',
    '''  addScipEvidence(generation, files, root, options, new Set([normalizePath(file.path)]), true);
  addBridgeEvidence(generation, selected);
  augmentStructuralIntelligence(generation, selected);
  augmentFrameworkIntelligence(generation, selected);
  attachPortableIdentities(generation);
  return generation;
''',
)

write("blueprint/tests/intelligence-build-integration.test.mjs", r'''import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

test("production graph build runs structural/framework/portable/convention layers", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-intelligence-build-"));
  try {
    mkdirSync(join(root, "src"), { recursive: true });
    writeFileSync(join(root, "src", "base.ts"), "export class Base { run() {} }\n");
    writeFileSync(join(root, "src", "child.ts"), `import { Base } from './base.js';\nexport class Child extends Base { run() {} }\nconst endpoint = process.env.API_URL;\n`);
    writeFileSync(join(root, "src", "tools.ts"), `export function ping() {}\nmcp.tool("ping", ping);\n`);
    const generation = buildGraphGeneration(root);
    const providers = generation.augmentation?.providers;
    assert.ok(providers?.structuralIntelligence);
    assert.ok(providers?.frameworkIntelligence);
    assert.ok(providers?.portableIdentity);
    assert.equal(providers?.conventions?.policyAuthority, false);
    assert.ok(generation.nodes.some((node) => node.labels?.includes("ConfigKey") && node.name === "API_URL"));
    assert.ok(generation.nodes.some((node) => node.labels?.includes("ToolContract") && node.name === "ping"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
''')

tests("structural-intelligence.test.mjs", "framework-process-contracts.test.mjs", "portable-identity-reanchor.test.mjs", "intelligence-build-integration.test.mjs", "production-provider-cluster.test.mjs")
commit("feat(blueprint-runtime): wire structural and framework intelligence into graph build", [structural, build, "blueprint/tests/intelligence-build-integration.test.mjs"])

# ---------------------------------------------------------------------------
# 2. Existing application tools expose the projections; no new MCP tools.
# ---------------------------------------------------------------------------
service = "blueprint/src/lib/application/service.mjs"
replace_once(
    service,
    'import { changesSinceReference } from "../../graph/snapshots.mjs";\n',
    'import { changesSinceReference } from "../../graph/snapshots.mjs";\nimport { buildBm25CodeIndex } from "../../graph/bm25-code-index.mjs";\nimport { searchAstStructure } from "../../graph/ast-structural-search.mjs";\nimport { buildProcessProjection } from "../../graph/process-projection.mjs";\nimport { buildContractRegistry } from "../../graph/contract-registry.mjs";\nimport { projectSymbolSignatures } from "../../graph/signature-projection.mjs";\nimport { buildColdStartOrientation } from "../../graph/orientation.mjs";\nimport { buildProjectionDependencyDag, ProjectionCache } from "../../graph/dependency-dag.mjs";\nimport { reanchorEvidence } from "../../graph/reanchor.mjs";\nimport { crossCheckWithLiveVerifier } from "../../providers/semantic-orchestrator.mjs";\n',
)
replace_once(
    service,
    '''  freshnessOwnership = "one_shot",
  buildSingleflight = createBuildSingleflight(),
} = {}) {
''',
    '''  freshnessOwnership = "one_shot",
  buildSingleflight = createBuildSingleflight(),
  liveVerifier = null,
  projectionCache = new ProjectionCache(),
} = {}) {
''',
)
replace_once(
    service,
    '''  const resolveRoot = (input = {}) => {
    if (rootRegistry) return rootRegistry.resolve(input);
    if (!allowEmbeddedRoot) return new RootRegistry().resolve(input);
    return resolve(input.repoRoot ?? process.cwd());
  };

''',
    '''  const resolveRoot = (input = {}) => {
    if (rootRegistry) return rootRegistry.resolve(input);
    if (!allowEmbeddedRoot) return new RootRegistry().resolve(input);
    return resolve(input.repoRoot ?? process.cwd());
  };

  function projectionDag(meta, generation) {
    return buildProjectionDependencyDag({
      sourceHash: meta?.manifest?.repo?.sourceHash ?? null,
      providerDigest: meta?.manifest?.manifestDigest ?? null,
      configDigest: generation?.augmentation?.configDigest ?? null,
      schemaVersion: meta?.schemaVersion ?? generation?.schemaVersion ?? null,
      generationId: meta?.manifest?.generationId ?? generation?.manifest?.generationId ?? null,
    });
  }

  function cachedProjection(name, db, meta, builder) {
    const generation = loadGeneration(db);
    const dag = projectionDag(meta, generation);
    const cached = projectionCache.getOrBuild(name, dag, () => builder(generation));
    return { generation, dag, ...cached };
  }

''',
)
old_search = '''    async search(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const query = String(input.query ?? "").trim();
        const generation = indexedQueryGeneration(db, query, { limit: Number(input.limit ?? 20), anchors: input.anchors ?? [] });
        const filtered = suppressRows(queryGraph(generation, { query, limit: Number(input.limit ?? 20) }), receipt, "search");
        return {
          schemaVersion: 1,
          kind: "search",
          generationId: meta.manifest.generationId,
          provider: meta.provider,
          query,
          results: filtered.rows,
          omissions: filtered.omissions,
          truncated: false,
          continuationCursor: null,
          freshnessReceipt: receipt,
        };
      }, options);
    },
'''
new_search = '''    async search(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const query = String(input.query ?? "").trim();
        const limit = Math.max(1, Math.min(200, Number(input.limit ?? 20) || 20));
        const indexed = indexedQueryGeneration(db, query, { limit, anchors: input.anchors ?? [] });
        const exact = suppressRows(queryGraph(indexed, { query, limit }), receipt, "search");
        const bm25Projection = cachedProjection("bm25", db, meta, (full) => buildBm25CodeIndex(full));
        const bm25Rows = suppressRows(
          bm25Projection.value.search(query, { limit }).map((row) => ({ ...row.document.node, lexicalScore: row.score, lexicalExactName: row.exactName })),
          receipt,
          "search_bm25",
        );
        const seen = new Set();
        const results = [];
        for (const row of [...exact.rows, ...bm25Rows.rows]) {
          if (!row?.id || seen.has(row.id)) continue;
          seen.add(row.id);
          results.push(row);
          if (results.length >= limit) break;
        }
        const structural = input.astPattern
          ? searchAstStructure(bm25Projection.generation, input.astPattern, { limit })
          : null;
        const structuralRows = structural ? suppressRows(structural.nodes, receipt, "search_structural") : { rows: [], omissions: [] };
        return {
          schemaVersion: 1,
          kind: "search",
          generationId: meta.manifest.generationId,
          provider: meta.provider,
          query,
          results,
          retrieval: {
            exactCount: exact.rows.length,
            bm25: { cache: bm25Projection.cache, fingerprint: bm25Projection.fingerprint, candidateCount: bm25Rows.rows.length },
            structural: structural ? { ...structural, nodes: structuralRows.rows } : null,
          },
          omissions: [...exact.omissions, ...bm25Rows.omissions, ...structuralRows.omissions],
          truncated: false,
          continuationCursor: null,
          freshnessReceipt: receipt,
        };
      }, options);
    },
'''
replace_once(service, old_search, new_search)
old_resolve = '''    async resolve(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const result = indexedResolve(db, String(input.nodeId ?? ""), {
          sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
        });
        if (!result) fail("node_not_found", `Graph node not found: ${input.nodeId}`);
        if (staleRow(result.node, staleSourcePolicy(receipt))) {
          fail("stale_source_suppressed", `Graph node is stale relative to current source: ${input.nodeId}`, { freshnessReceipt: receipt });
        }
        return { ...result, generationId: meta.manifest.generationId, freshnessReceipt: receipt };
      }, options);
    },
'''
new_resolve = '''    async resolve(input = {}, options = {}) {
      return withCurrentDb(input, async ({ db, meta, receipt }) => {
        let result = indexedResolve(db, String(input.nodeId ?? ""), {
          sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
        });
        let reanchor = null;
        if (!result && input.previousEvidence) {
          const current = loadGeneration(db);
          reanchor = reanchorEvidence(input.previousEvidence, current.nodes ?? []);
          if (reanchor.state === "ambiguous") fail("anchor_ambiguous", "Previous evidence re-anchors to more than one current fact.", { reanchor });
          if (reanchor.state === "reanchored") {
            result = indexedResolve(db, reanchor.targetId, {
              sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
            });
          }
        }
        if (!result) fail("node_not_found", `Graph node not found: ${input.nodeId}`);
        if (staleRow(result.node, staleSourcePolicy(receipt))) {
          fail("stale_source_suppressed", `Graph node is stale relative to current source: ${input.nodeId}`, { freshnessReceipt: receipt });
        }
        const verification = input.verifySemantic === true
          ? await crossCheckWithLiveVerifier({ canonical: result.node, verifier: liveVerifier, request: input, sourceStateId: meta.manifest.generationId, signal: options.signal })
          : null;
        return { ...result, reanchor, verification, generationId: meta.manifest.generationId, freshnessReceipt: receipt };
      }, options);
    },
'''
replace_once(service, old_resolve, new_resolve)
replace_once(
    service,
    '''        if (view === "projection") {
''',
    '''        if (view === "processes") {
          const cached = cachedProjection("processes", db, meta, (generation) => buildProcessProjection(generation, {
            maxProcesses: input.maxProcesses,
            maxDepth: input.maxDepth,
            maxSteps: input.maxSteps,
          }));
          return { ...cached.value, cache: cached.cache, freshnessReceipt: receipt };
        }
        if (view === "contracts") {
          const cached = cachedProjection("contracts", db, meta, (generation) => buildContractRegistry(generation, { repoId: input.repoId ?? null }));
          return { ...cached.value, cache: cached.cache, freshnessReceipt: receipt };
        }
        if (view === "signatures") {
          const cached = cachedProjection("signatures", db, meta, (generation) => projectSymbolSignatures(generation, { limit: input.limit, pathPrefix: input.pathPrefix, kinds: input.kinds }));
          return { ...cached.value, cache: cached.cache, freshnessReceipt: receipt };
        }
        if (view === "orientation") {
          const cached = cachedProjection("orientation", db, meta, (generation) => {
            const files = (generation.nodes ?? []).filter((node) => node.kind === "file").map((node) => ({ path: node.path }));
            return buildColdStartOrientation(generation, files, {
              signatureLimit: input.signatureLimit,
              entryPointLimit: input.entryPointLimit,
              contractLimit: input.contractLimit,
            });
          });
          return { ...cached.value, cache: cached.cache, freshnessReceipt: receipt };
        }
        if (view === "projection") {
''',
)
replace_once(
    service,
    '''        if (view !== "summary") fail("architecture_view_invalid", "Architecture view must be summary, flows, liveness, projection, or changes.");
''',
    '''        if (view !== "summary") fail("architecture_view_invalid", "Architecture view must be summary, flows, liveness, processes, contracts, signatures, orientation, projection, or changes.");
''',
)
replace_once(
    service,
    '''    async federate(input = {}, options = {}) {
      const repositories = input.repositories ?? [];
      const operation = String(input.operation ?? "recall");
      const allowedRepoIds = input.allowedRepoIds ?? repositories.map((repository) => repository.repoId);
      return routeFederatedQuery({
        repositories,
        allowedRepoIds,
''',
    '''    async federate(input = {}, options = {}) {
      const group = input.group ?? null;
      const repositories = group?.repositories ?? input.repositories ?? [];
      const operation = String(input.operation ?? "recall");
      const allowedRepoIds = input.allowedRepoIds ?? repositories.map((repository) => repository.repoId);
      return routeFederatedQuery({
        group,
        repositories,
        allowedRepoIds,
''',
)
replace_once(
    service,
    '''            freshnessOwnership,
            buildSingleflight,
          });
''',
    '''            freshnessOwnership,
            buildSingleflight,
            liveVerifier,
          });
''',
)

# Make live verification treat node identity as an exact canonical target too.
semantic = "blueprint/src/providers/semantic-orchestrator.mjs"
replace_once(
    semantic,
    '''    const verification = liveVerificationCandidate(canonical, raw, sourceStateId);
    const evaluated = evaluateEvidence({
      targetSourceState: sourceStateId ?? canonical.sourceStateId ?? canonical.generationId ?? null,
      candidates: [canonical, verification],
      requestedRelation: canonical.relation ?? null,
    });
''',
    '''    const canonicalTarget = canonical.targetId ?? canonical.target ?? canonical.id ?? null;
    const canonicalForEvaluation = canonical.targetId || canonical.target
      ? canonical
      : {
          ...canonical,
          relation: canonical.relation ?? "DEFINES",
          source: canonical.source ?? canonical.id,
          target: canonicalTarget,
          targetId: canonicalTarget,
          sourceStateId: canonical.sourceStateId ?? sourceStateId,
          sourceRelation: canonical.sourceRelation ?? "current",
          provenance: canonical.provenance ?? FACT_PROVENANCE.RULE_RESOLVED,
          confidenceTier: canonical.confidenceTier ?? "EXACT_RESOLUTION",
          resolved: Boolean(canonicalTarget),
        };
    const verification = liveVerificationCandidate(canonicalForEvaluation, raw, sourceStateId);
    const evaluated = evaluateEvidence({
      targetSourceState: sourceStateId ?? canonicalForEvaluation.sourceStateId ?? canonicalForEvaluation.generationId ?? null,
      candidates: [canonicalForEvaluation, verification],
      requestedRelation: canonicalForEvaluation.relation ?? null,
    });
''',
)

write("blueprint/tests/application-v2-projections.test.mjs", r'''import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";

function repo() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-app-v2-"));
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "service.ts"), `export function placeOrder() { return 1; }\nexport function main() { return placeOrder(); }\n`);
  buildGraphGeneration(root, { outDir: ".agent", persist: true });
  return root;
}

test("existing search tool exposes BM25 and structural retrieval without a new MCP tool", async () => {
  const root = repo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const result = await service.search({ repoRoot: root, query: "place order", astPattern: { kind: "Function", name: "place" }, limit: 10 });
    assert.ok(result.retrieval?.bm25?.fingerprint);
    assert.ok(result.results.some((row) => row.name === "placeOrder"));
    assert.ok(result.retrieval.structural);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("architecture tool exposes orientation process contract and signature projections", async () => {
  const root = repo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const orientation = await service.architecture({ repoRoot: root, view: "orientation" });
    const processes = await service.architecture({ repoRoot: root, view: "processes" });
    const contracts = await service.architecture({ repoRoot: root, view: "contracts" });
    const signatures = await service.architecture({ repoRoot: root, view: "signatures", limit: 20 });
    assert.equal(orientation.kind, "cold-start-orientation");
    assert.equal(processes.kind, "process-projection");
    assert.ok(Array.isArray(contracts.contracts));
    assert.ok(signatures.signatures.some((row) => row.name === "placeOrder"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("resolve can request an on-demand live semantic cross-check without replacing canonical identity", async () => {
  const root = repo();
  try {
    const service = createBlueprintApplicationService({
      allowEmbeddedRoot: true,
      liveVerifier: async ({ canonical }) => ({ provider: "lsp-test", entityId: canonical.id }),
    });
    const search = await service.search({ repoRoot: root, query: "placeOrder", limit: 5 });
    const symbol = search.results.find((row) => row.name === "placeOrder");
    const resolved = await service.resolve({ repoRoot: root, nodeId: symbol.id, verifySemantic: true });
    assert.equal(resolved.node.id, symbol.id);
    assert.equal(resolved.verification.state, "agreement");
  } finally { rmSync(root, { recursive: true, force: true }); }
});
''')

tests("application-v2-projections.test.mjs", "application-service-queries.test.mjs", "semantic-orchestrator.test.mjs", "retrieval-projections.test.mjs", "dependency-dag.test.mjs")
commit("feat(blueprint-application): expose v2 intelligence through existing tools", [service, semantic, "blueprint/tests/application-v2-projections.test.mjs"])

# ---------------------------------------------------------------------------
# 3. Named federation groups and exact contract bridges/traces.
# ---------------------------------------------------------------------------
federation = "blueprint/src/lib/federation/index.mjs"
write(federation, r'''// Scoped federation: repositories retain independent generation/identity spaces.
// Named groups are configuration only; cross-repo traversal crosses exact
// contract bridges and never same-name similarity joins.

import { BlueprintError } from "../application/errors.mjs";
import { stitchContractTraces } from "../../graph/contract-registry.mjs";

export function defineFederationGroup({ name, repositories } = {}) {
  const groupName = String(name ?? "").trim();
  if (!groupName) throw new BlueprintError("federation_group_name_invalid", "federation group requires a non-empty name");
  if (!Array.isArray(repositories) || repositories.length < 1 || repositories.length > 16) throw new BlueprintError("federation_bounds_invalid", "federation group requires 1 to 16 repositories");
  const ids = new Set();
  const normalized = repositories.map((repository) => {
    if (!repository?.repoId || ids.has(repository.repoId)) throw new BlueprintError("repository_duplicate", "each federated repository needs one unique repoId");
    ids.add(repository.repoId);
    return Object.freeze({ ...repository });
  });
  return Object.freeze({ schemaVersion: 1, name: groupName, repositories: Object.freeze(normalized) });
}

export function composeFederatedSlices(slices = [], { groupName = null } = {}) {
  const repos = [];
  const seen = new Set();
  for (const slice of slices) {
    if (!slice.repoId || !slice.generationId) throw new BlueprintError("slice_incomplete", "each federated slice needs repoId and generationId");
    if (repos.some((existing) => existing.repoId === slice.repoId && existing.generationId !== slice.generationId)) throw new BlueprintError("generation_ambiguity", `repo ${slice.repoId} contributed two generations`);
    if (seen.has(slice.repoId)) throw new BlueprintError("repository_duplicate", `repo ${slice.repoId} contributed more than one slice`);
    seen.add(slice.repoId);
    repos.push({ repoId: slice.repoId, repoRoot: slice.repoRoot ?? null, generationId: slice.generationId, receiptId: slice.receiptId ?? null, resultCount: slice.results?.length ?? 0 });
  }
  const registries = slices.filter((slice) => Array.isArray(slice.contracts) && slice.contracts.length).map((slice) => ({ schemaVersion: 1, repoId: slice.repoId, generationId: slice.generationId, contracts: slice.contracts }));
  const stitching = stitchContractTraces(registries);
  return {
    schemaVersion: 1,
    kind: "federated",
    groupName,
    repos,
    plannerAuthority: "external",
    selection: "unranked_repository_slices",
    slices: slices.map((slice) => ({ repoId: slice.repoId, repoRoot: slice.repoRoot ?? null, generationId: slice.generationId, receiptId: slice.receiptId ?? null, results: slice.results ?? [], omissions: slice.omissions ?? [], contracts: slice.contracts ?? [] })),
    results: slices.map((slice) => ({ repoId: slice.repoId, generationId: slice.generationId, results: slice.results ?? [] })),
    contractBridges: stitching.bridges,
    traces: stitching.traces,
  };
}

export function isRepoAllowed(slice, allowedRepoIds) { return allowedRepoIds.includes(slice.repoId); }

export async function routeFederatedQuery({ group = null, repositories = [], allowedRepoIds = [], operation, input = {}, querySlice }) {
  const normalizedGroup = group ? defineFederationGroup(group) : null;
  const selected = normalizedGroup?.repositories ?? repositories;
  if (!Array.isArray(selected) || selected.length < 1 || selected.length > 16) throw new BlueprintError("federation_bounds_invalid", "federation requires 1 to 16 explicit repositories");
  if (typeof querySlice !== "function") throw new BlueprintError("federation_router_missing", "federation query router is unavailable");
  if (!new Set(["search", "recall", "impact", "architecture"]).has(operation)) throw new BlueprintError("federation_operation_invalid", `unsupported federated operation ${operation}`);
  const ids = new Set();
  for (const repository of selected) {
    if (!repository?.repoId || ids.has(repository.repoId)) throw new BlueprintError("repository_duplicate", "each federated repository needs one unique repoId");
    ids.add(repository.repoId);
    if (!isRepoAllowed(repository, allowedRepoIds)) throw new BlueprintError("repository_not_allowed", `repo ${repository.repoId} is outside the explicit federation allowlist`);
  }
  const slices = await Promise.all(selected.map(async (repository) => {
    try {
      const result = await querySlice(repository, operation, input);
      return { repoId: repository.repoId, repoRoot: result.repoRoot ?? repository.repoRoot ?? null, generationId: result.generationId, receiptId: result.freshnessReceipt?.receiptId ?? null, results: [result], omissions: [], contracts: result.contracts ?? [] };
    } catch (error) {
      return { repoId: repository.repoId, repoRoot: repository.repoRoot ?? null, generationId: repository.generation ?? "unavailable", receiptId: null, results: [], omissions: [{ reason: "repository_query_failed", code: error?.code ?? "internal_error", message: error?.message ?? String(error) }], contracts: [] };
    }
  }));
  return composeFederatedSlices(slices, { groupName: normalizedGroup?.name ?? null });
}
''')

write("blueprint/tests/federation-groups-contracts.test.mjs", r'''import assert from "node:assert/strict";
import test from "node:test";
import { composeFederatedSlices, defineFederationGroup, routeFederatedQuery } from "../src/lib/federation/index.mjs";

const provider = { contractId: "c", contractKey: "sha256:k", repoId: "provider", kind: "tool", address: "ping", schema: null, roles: ["provider"], nodeId: "tool:ping", evidence: [] };
const consumer = { ...provider, repoId: "consumer", roles: ["consumer"], nodeId: "call:ping" };

test("named federation groups validate unique bounded repository membership", () => {
  const group = defineFederationGroup({ name: "payments", repositories: [{ repoId: "a" }, { repoId: "b" }] });
  assert.equal(group.name, "payments");
  assert.deepEqual(group.repositories.map((r) => r.repoId), ["a", "b"]);
  assert.throws(() => defineFederationGroup({ name: "bad", repositories: [{ repoId: "a" }, { repoId: "a" }] }), { code: "repository_duplicate" });
});

test("federated slices stitch only exact contract bridges without merging node spaces", () => {
  const result = composeFederatedSlices([
    { repoId: "consumer", generationId: "g1", results: [], contracts: [consumer] },
    { repoId: "provider", generationId: "g2", results: [], contracts: [provider] },
  ], { groupName: "tools" });
  assert.equal(result.groupName, "tools");
  assert.equal(result.contractBridges.length, 1);
  assert.equal(result.traces.length, 1);
  assert.deepEqual(result.traces[0].steps.map((step) => step.repoId), ["consumer", "provider"]);
  assert.equal(result.slices.length, 2);
});

test("routeFederatedQuery accepts a named group and preserves per-repo generations", async () => {
  const result = await routeFederatedQuery({
    group: { name: "g", repositories: [{ repoId: "a", generation: "ga" }, { repoId: "b", generation: "gb" }] },
    allowedRepoIds: ["a", "b"],
    operation: "architecture",
    input: { view: "contracts" },
    querySlice: async (repo) => ({ generationId: repo.generation, contracts: [] }),
  });
  assert.equal(result.groupName, "g");
  assert.deepEqual(result.repos.map((repo) => repo.generationId), ["ga", "gb"]);
});
''')

tests("federation-groups-contracts.test.mjs", "evidence-pack.test.mjs", "application-v2-projections.test.mjs")
commit("feat(blueprint-federation): add named groups and exact contract trace stitching", [federation, "blueprint/tests/federation-groups-contracts.test.mjs"])

# ---------------------------------------------------------------------------
# Restore read-only completion CI and remove this one-use transport.
# ---------------------------------------------------------------------------
workflow = ROOT / ".github/workflows/blueprint-completion.yml"
text = workflow.read_text()
text = text.replace("permissions:\n  contents: write\n", "permissions:\n  contents: read\n", 1)
step = '''      - name: Apply reviewed Blueprint integration wave
        run: python3 .github/blueprint-completion-input/apply-wave2.py
'''
if text.count(step) != 1:
    raise SystemExit("temporary integration step missing")
workflow.write_text(text.replace(step, "", 1))
subprocess.run(["git", "rm", ".github/blueprint-completion-input/apply-wave2.py"], cwd=ROOT, check=True)
subprocess.run(["git", "add", ".github/workflows/blueprint-completion.yml"], cwd=ROOT, check=True)
subprocess.run(["git", "commit", "-m", "ci(blueprint): remove runtime integration transport"], cwd=ROOT, check=True)
subprocess.run(["git", "push", "origin", "HEAD:blueprint-completion"], cwd=ROOT, check=True)
