import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { observeCurrentSourceAtPath, syncToCurrentSourceAtPath } from "../../graph/barrier.mjs";
import {
  closeStore,
  listClaimSlice,
  listDocumentSupersession,
  loadGeneration,
  openStoreReadOnly,
} from "../../graph/store-sqlite.mjs";
import {
  boundedArchitecture,
  boundedImpact,
  boundedNeighbors,
  boundedPath,
  indexedQueryGeneration,
  indexedResolve,
  readIndexedMeta,
} from "../../graph/traverse-store.mjs";
import {
  graphStatus,
  graphFlowInventory,
  queryGraph,
  repositoryIdentity,
} from "../../graph/static-provider.mjs";
import { executeRecallCircuit, recallCircuitToCandidateSet } from "../../graph/recall-circuit.mjs";
import { assertGenerationCoherence, buildFreshnessReceipt } from "../../graph/freshness-receipt.mjs";
import { resolveSeeds } from "../../graph/seed-resolver.mjs";
import { resolveImpactSeedEnvelope } from "../../graph/analytics/change-impact.mjs";
import { decomposeChangeRisk } from "../../graph/analytics/index.mjs";
import { buildDisposableArchitectureProjection } from "../../graph/architecture-model.mjs";
import { projectDocumentTruth } from "../../graph/doc-truth-projection.mjs";
import { buildLivenessProjection } from "../../graph/liveness.mjs";
import { recommendTestsForImpact } from "../../graph/test-recommendation.mjs";
import { changesSinceReference } from "../../graph/snapshots.mjs";
import { buildBm25CodeIndex } from "../../graph/bm25-code-index.mjs";
import { searchAstStructure } from "../../graph/ast-structural-search.mjs";
import { buildProcessProjection } from "../../graph/process-projection.mjs";
import { buildContractRegistry } from "../../graph/contract-registry.mjs";
import { projectSymbolSignatures } from "../../graph/signature-projection.mjs";
import { buildColdStartOrientation } from "../../graph/orientation.mjs";
import { buildProjectionDependencyDag, ProjectionCache } from "../../graph/dependency-dag.mjs";
import { reanchorEvidence } from "../../graph/reanchor.mjs";
import { crossCheckWithLiveVerifier } from "../../providers/semantic-orchestrator.mjs";
import { routeFederatedQuery } from "../federation/index.mjs";
import { observeRepositoryFreshness } from "../../sources/freshness-observation.mjs";
import { serviceStatus } from "../../service/status.mjs";
import { createBuildSingleflight } from "../../service/build-singleflight.mjs";
import { RootRegistry } from "./root-registry.mjs";
import { fail } from "./errors.mjs";

function databasePath(root, outDir) {
  return join(root, outDir, "graph", "graph.db");
}

function throwIfAborted(signal) {
  if (signal?.aborted) fail("request_cancelled", "request cancelled");
}

function staleSourcePolicy(receipt) {
  const freshness = receipt?.freshness ?? "unknown";
  const stale = receipt?.suppression?.required === true;
  return {
    stale,
    wholeGeneration: stale && receipt?.suppression?.mode === "whole_generation",
    paths: new Set((receipt?.staleSources?.paths ?? []).map((path) => String(path).replaceAll("\\", "/"))),
  };
}

function rowPaths(row) {
  const paths = [row?.path, row?.sourceRef?.split(":")[0], ...(row?.evidence ?? []).map((item) => item?.path), ...(row?.citations ?? []).map((item) => item?.path)];
  return paths.filter(Boolean).map((path) => String(path).replaceAll("\\", "/"));
}

function staleRow(row, policy) {
  if (!policy.stale) return false;
  if (policy.wholeGeneration) return true;
  return rowPaths(row).some((path) => policy.paths.has(path));
}

function suppressRows(rows, receipt, lane) {
  const policy = staleSourcePolicy(receipt);
  const kept = [], suppressed = [];
  for (const row of rows ?? []) (staleRow(row, policy) ? suppressed : kept).push(row);
  return {
    rows: kept,
    omissions: suppressed.length || policy.wholeGeneration
      ? [{ reason: "stale_source_suppressed", lane, count: suppressed.length, scope: policy.wholeGeneration ? "whole_generation" : "changed_paths" }]
      : [],
  };
}

function suppressTraversalPayload(payload, receipt) {
  const nodeField = payload.path ? "path" : payload.impacted ? "impacted" : payload.nodes ? "nodes" : payload.examples ? "examples" : null;
  const nodeRows = nodeField ? payload[nodeField] : [];
  const filtered = suppressRows(nodeRows, receipt, payload.kind ?? "traversal");
  const allowed = new Set(filtered.rows.map((row) => row.id));
  if (payload.root) allowed.add(payload.root);
  const edges = (payload.edges ?? []).filter((edge) => allowed.has(edge.source) && allowed.has(edge.target));
  const next = { ...payload, omissions: [...(payload.omissions ?? []), ...filtered.omissions], edges };
  if (nodeField) next[nodeField] = filtered.rows;
  if (payload.target && staleRow(payload.target, staleSourcePolicy(receipt))) {
    next.target = null;
    next.omissions.push({ reason: "stale_source_suppressed", lane: "target", count: 1 });
  }
  return next;
}

function flowCursor(generationId, offset) {
  return Buffer.from(JSON.stringify({ view: "flows", generationId, offset })).toString("base64url");
}

function decodeFlowCursor(cursor, generationId) {
  if (!cursor) return 0;
  try {
    const value = JSON.parse(Buffer.from(String(cursor), "base64url").toString("utf8"));
    if (value.view !== "flows" || value.generationId !== generationId || !Number.isSafeInteger(value.offset) || value.offset < 0 || value.offset > 10000) {
      fail("cursor_invalid", "Architecture flow cursor does not match the served generation.");
    }
    return value.offset;
  } catch (error) {
    if (error?.code === "cursor_invalid") throw error;
    fail("cursor_invalid", "Architecture flow cursor is malformed.");
  }
}

function architectureFlowPage(db, meta, input, freshness) {
  const generationId = meta.manifest.generationId;
  const offset = decodeFlowCursor(input.cursor, generationId);
  const requestedMaxFlows = Number(input.maxFlows ?? input.limit ?? 50);
  if (!Number.isSafeInteger(requestedMaxFlows) || requestedMaxFlows < 1 || requestedMaxFlows > 200) {
    fail("architecture_bounds_invalid", "Architecture flow maxFlows must be an integer from 1 to 200.");
  }
  const maxFlows = requestedMaxFlows;
  const inventory = graphFlowInventory(loadGeneration(db), {
    complete: false,
    maxFlows: offset + maxFlows + 1,
  });
  const compactNode = (node) => {
    const evidence = node?.evidence?.[0] ?? {};
    return { id: node.id, kind: node.kind, path: node.path ?? evidence.path ?? null, startLine: evidence.startLine ?? null, endLine: evidence.endLine ?? null };
  };
  const flows = inventory.flows.slice(offset, offset + maxFlows).map((flow) => ({
    id: flow.id ?? `flow:${flow.entry.id}:broken`,
    status: flow.status,
    entry: compactNode(flow.entry),
    ...(flow.terminal ? { terminal: compactNode(flow.terminal) } : {}),
    path: flow.path.map(compactNode),
    ...(flow.missingHop ? { missingHop: flow.missingHop } : {}),
    evidence: flow.evidence.map((item) => ({ path: item.path, startLine: item.startLine ?? null, endLine: item.endLine ?? null, contentHash: item.contentHash ?? null })),
  }));
  const truncated = inventory.flows.length > offset + flows.length;
  return {
    schemaVersion: 2,
    provider: inventory.provider,
    kind: "architecture",
    view: "flows",
    ...freshness,
    ordering: "entry.id,path[].id",
    bounds: { maxFlows, maxDepth: 12 },
    entryPoints: inventory.entryPoints,
    flows,
    truncated,
    continuationCursor: truncated ? flowCursor(generationId, offset + flows.length) : null,
  };
}

export function createBlueprintApplicationService({
  outDir = ".agent",
  rootRegistry = null,
  allowEmbeddedRoot = false,
  freshnessOwnership = "one_shot",
  buildSingleflight = createBuildSingleflight(),
  liveVerifier = null,
  projectionCache = new ProjectionCache(),
} = {}) {
  if (!["one_shot", "resident"].includes(freshnessOwnership)) {
    throw new TypeError("freshnessOwnership must be one_shot or resident");
  }
  const resolveRoot = (input = {}) => {
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

  async function openFreshnessSession(input = {}, { signal } = {}) {
    throwIfAborted(signal);
    const root = resolveRoot(input);
    const dbPath = databasePath(root, outDir);
    const initialized = !existsSync(dbPath);
    if (initialized) {
      const build = await buildSingleflight.build({
        root,
        outDir,
        // Ordinary direct requests initialize Phase 1 graph state only. Phase
        // 2 semantic synthesis remains an explicit workflow.
        options: { noReadmeLink: true },
      }, { signal });
      if (build.exitCode !== 0 || !existsSync(dbPath)) {
        fail("graph_initialization_failed", `Blueprint initial graph build failed for ${root}.`, {
          root,
          exitCode: build.exitCode,
        });
      }
    }
    let receipt;
    try {
      // Initial build is Phase 1 publication from this request's current root.
      // Every concurrent first request joins its build, then reads its sealed
      // result. Do not turn that retry into competing one-shot writer barriers.
      receipt = initialized
        ? { receiptId: `initial-build-${Date.now()}`, barrierResult: "caught_up", initialBuild: true }
        : freshnessOwnership === "resident"
        ? observeCurrentSourceAtPath(root, { outDir })
        : await syncToCurrentSourceAtPath(root, { outDir, timeoutMs: Number(input.timeoutMs ?? 2000), signal });
    } catch (error) {
      if (error?.code === "request_cancelled") fail("request_cancelled", "request cancelled");
      throw error;
    }
    throwIfAborted(signal);
    const db = openStoreReadOnly(dbPath);
    let closed = false;
    let snapshotOpen = false;
    const closeSnapshot = () => {
      if (closed) return;
      closed = true;
      try { if (snapshotOpen) db.exec("ROLLBACK;"); }
      finally { closeStore(db); }
    };
    try {
      // Pin the first envelope read and every subsequent fact query to one
      // committed WAL snapshot. Concurrent writers may publish a new graph;
      // this session must never mix its rows with the old generation receipt.
      db.exec("BEGIN;");
      snapshotOpen = true;
      const meta = readIndexedMeta(db);
      if (!meta?.manifest?.generationId) fail("graph_missing", "No sealed generation is available.");
      if (meta.schemaVersion !== 1 || (!meta.provider || (typeof meta.provider !== "string" && typeof meta.provider.id !== "string")) || !meta.manifest.manifestDigest) {
        fail("schema_mismatch", "Sealed Blueprint generation schema does not match the current service.", {
          schemaVersion: meta.schemaVersion ?? null,
          provider: meta.provider ?? null,
          manifestDigest: meta.manifest.manifestDigest ?? null,
        });
      }
      try {
        assertGenerationCoherence({
          pinnedGenerationId: input.generation,
          servedGenerationId: meta.manifest.generationId,
        });
      } catch (error) {
        if (error?.code === "generation_mismatch") {
          fail(error.code, error.message, error.details);
        }
        throw error;
      }
      // Preserve the existing barrier fields for compatibility while making
      // BlueprintFreshnessReceiptV1 the production receipt. Freshness and
      // generation coherence remain independent axes.
      const canonicalFreshness = buildFreshnessReceipt(db, root);
      const freshnessReceipt = Object.freeze({
        ...receipt,
        ...canonicalFreshness,
        // A resident reader never repairs. Direct source observation is the
        // final freshness gate even if watcher clocks have not yet advanced.
        ...(freshnessOwnership === "resident" && canonicalFreshness.freshness !== "fresh"
          ? { barrierResult: "timeout" }
          : {}),
        barrier: receipt,
      });
      if (freshnessReceipt.barrierResult !== "caught_up" && !input.allowStale) {
        fail("stale_blocked", "Blueprint freshness barrier did not catch up.", { receipt: freshnessReceipt });
      }
      return Object.freeze({
        root,
        db,
        meta,
        receipt: freshnessReceipt,
        get closed() { return closed; },
        close: closeSnapshot,
      });
    } catch (error) {
      closeSnapshot();
      throw error;
    }
  }

  async function withCurrentDb(input, callback, { signal, session = null } = {}) {
    const current = session ?? await openFreshnessSession(input, { signal });
    try {
      if (current.closed) fail("session_closed", "Blueprint freshness session is already closed.");
      throwIfAborted(signal);
      const result = await callback(current);
      throwIfAborted(signal);
      return result;
    } finally {
      if (!session) current.close();
    }
  }

  async function resolveAnchor(input, options) {
    return withCurrentDb(input, ({ db, meta }) => {
      const anchor = String(input.anchor ?? "").trim();
      const resolution = resolveSeeds(db, anchor, {
        generationId: meta.manifest.generationId,
        seedIds: anchor.startsWith("symbol:") ? [anchor] : [],
        anchors: [anchor],
        maxSeeds: 8,
      });
      if (resolution.state === "resolved" && resolution.seeds.length === 1) {
        return { nodeId: resolution.seeds[0].id, resolution };
      }
      if (resolution.state === "ambiguous") fail("anchor_ambiguous", `More than one graph anchor matches ${anchor}.`, { resolution });
      fail("anchor_not_found", `No graph anchor matches ${anchor}.`, { resolution });
    }, options);
  }

  return Object.freeze({
    resolveCanonicalRoot(input = {}) {
      return resolveRoot(input);
    },
    openFreshnessSession,
    async status(input = {}, { signal } = {}) {
      throwIfAborted(signal);
      const root = resolveRoot(input);
      const status = graphStatus(root, outDir);
      const overlay = observeRepositoryFreshness(root, {
        baseCommit: status.manifest?.repo?.baseCommit ?? null,
      });
      const runtimeStatus = serviceStatus({ target: root });
      return {
        schemaVersion: 1,
        repository: {
          ...repositoryIdentity(root),
          revision: overlay.revision,
        },
        overlay,
        runtime: {
          watcherRunning: runtimeStatus.running,
          enrolledRepoCount: runtimeStatus.enrolledRepos.length,
        },
        ...status,
      };
    },

    async search(input = {}, options = {}) {
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

    async resolve(input = {}, options = {}) {
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

    async recall(input = {}, options = {}) {
      return withCurrentDb(input, ({ root, db, meta, receipt }) => {
        const query = String(input.query ?? input.task ?? "").trim();
        const circuit = executeRecallCircuit(db, String(input.task ?? query), {
          generationId: meta.manifest.generationId,
          anchors: input.anchors ?? [],
          policy: input.policy,
          limits: { maxPaths: Number(input.limit ?? 40) },
        });
        const candidates = recallCircuitToCandidateSet(circuit, {
          provider: meta.provider,
          anchors: input.anchors ?? [],
          ...repositoryIdentity(root),
          repoRoot: root,
          receiptId: receipt.receiptId,
          indexedAt: meta.manifest.generatedAt,
          freshness: {
            revision: meta.manifest.generationId,
            indexedAt: meta.manifest.generatedAt,
            stale: receipt.freshness !== "fresh",
          },
          canonical: true,
        });
        const filtered = suppressRows(candidates.candidates, receipt, "recall_candidate");
        const candidateSet = { ...candidates, candidates: filtered.rows, omissions: [...(candidates.omissions ?? []), ...filtered.omissions] };
        const policy = staleSourcePolicy(receipt);
        const paths = (circuit.paths ?? []).filter((path) => !path.nodes.some((node) => staleRow(node, policy)));
        const suppressedPaths = (circuit.paths?.length ?? 0) - paths.length;
        const recallCircuit = suppressedPaths ? {
          ...circuit,
          paths,
          omissions: [...(circuit.omissions ?? []), { reason: "stale_source_suppressed", lane: "evidence_path", count: suppressedPaths }],
          state: paths.length ? circuit.state : "abstained",
        } : circuit;
        return {
          schemaVersion: 1,
          action: "allow",
          reasonCode: "recalled",
          generationId: meta.manifest.generationId,
          candidateSet,
          recallCircuit,
          freshnessReceipt: receipt,
          omissions: candidateSet.omissions ?? [],
        };
      }, options);
    },

    async expand(input = {}, options = {}) {
      const session = await openFreshnessSession(input, options);
      try {
        const scopedOptions = { ...options, session };
        const anchor = await resolveAnchor(input, scopedOptions);
        return withCurrentDb(input, ({ db, receipt }) => suppressTraversalPayload({ ...boundedNeighbors(db, {
          nodeId: anchor.nodeId,
          direction: input.direction ?? "both",
          depth: Number(input.depth ?? 1),
          budget: Number(input.budget ?? 2000),
          cursor: input.cursor,
          freshness: {
            generationId: receipt.generationId,
            sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
            dirtyFileCount: 0,
          },
        }), seedResolution: anchor.resolution }, receipt), scopedOptions);
      } finally {
        session.close();
      }
    },

    async impact(input = {}, options = {}) {
      const session = await openFreshnessSession(input, options);
      try {
        const scopedOptions = { ...options, session };
        return withCurrentDb(input, ({ root, db, meta, receipt }) => {
          const seedEnvelope = resolveImpactSeedEnvelope(db, root, meta.manifest.generationId, input);
          if (!seedEnvelope.seeds.length) fail("impact_seed_unresolved", "Impact requires at least one unambiguous seed.", { seedEnvelope });
          const slices = seedEnvelope.seeds.map((seed) => suppressTraversalPayload(boundedImpact(db, {
            nodeId: seed.id,
            depth: Number(input.depth ?? 3),
            budget: Number(input.budget ?? 2000),
            cursor: input.cursor,
            freshness: {
              generationId: receipt.generationId,
              sourceState: receipt.freshness === "fresh" ? "clean" : "stale",
              dirtyFileCount: receipt.staleSources?.paths?.length ?? 0,
            },
          }), receipt));
          const primary = slices[0];
          const impacted = slices.flatMap((slice) => slice.impacted ?? []);
          const edges = slices.flatMap((slice) => slice.edges ?? []);
          const risk = decomposeChangeRisk({
            changedPaths: seedEnvelope.changedPaths,
            impacted,
            edges,
            truncated: slices.some((slice) => slice.truncated),
            ambiguousSeeds: seedEnvelope.resolution.candidateCount > seedEnvelope.seeds.length ? seedEnvelope.resolution.candidateCount : 0,
            stale: receipt.freshness !== "fresh",
            cochangeScore: input.cochangeScore,
          });
          const testRecommendations = recommendTestsForImpact(db, {
            generationId: meta.manifest.generationId,
            impactedIds: [...seedEnvelope.seeds.map((seed) => seed.id), ...impacted.map((node) => node.id)].filter(Boolean),
            maxRecommendations: input.maxTestRecommendations,
          });
          return {
            ...primary,
            seedEnvelope,
            risk,
            slices,
            testRecommendations,
            omissions: [...(primary.omissions ?? []), ...seedEnvelope.omissions, ...testRecommendations.omissions],
          };
        }, scopedOptions);
      } finally {
        session.close();
      }
    },

    async path(input = {}, options = {}) {
      const fromInput = String(input.from ?? "").trim();
      const toInput = String(input.to ?? "").trim();
      if (!fromInput || !toInput) fail("path_endpoint_required", "Path requires non-empty from and to anchors.");
      const maxDepth = Number(input.maxDepth ?? 5);
      if (!Number.isSafeInteger(maxDepth) || maxDepth < 1 || maxDepth > 12) {
        fail("path_bounds_invalid", "Path maxDepth must be an integer from 1 to 12.");
      }
      const budget = Number(input.budget ?? 2000);
      if (!Number.isSafeInteger(budget) || budget < 128 || budget > 32000) {
        fail("path_bounds_invalid", "Path budget must be an integer from 128 to 32000.");
      }
      const session = await openFreshnessSession(input, options);
      try {
        const scopedOptions = { ...options, session };
        const from = await resolveAnchor({ ...input, anchor: fromInput }, scopedOptions);
        const to = await resolveAnchor({ ...input, anchor: toInput }, scopedOptions);
        return withCurrentDb(input, ({ db, receipt }) => suppressTraversalPayload({ ...boundedPath(db, {
          from: from.nodeId,
          to: to.nodeId,
          maxDepth,
          budget,
          cursor: input.cursor,
          freshness: {
            generationId: receipt.generationId,
            sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
            dirtyFileCount: 0,
          },
        }), seedResolution: { from: from.resolution, to: to.resolution } }, receipt), scopedOptions);
      } finally {
        session.close();
      }
    },

    async architecture(input = {}, options = {}) {
      return withCurrentDb(input, ({ root, db, meta, receipt }) => {
        const freshness = {
          generationId: receipt.generationId,
          sourceState: receipt.freshness === "fresh" ? "clean" : "stale",
          dirtyFileCount: receipt.staleSources?.paths?.length ?? 0,
        };
        const view = input.view ?? "summary";
        if (view === "flows") {
          const payload = architectureFlowPage(db, meta, input, freshness);
          const filtered = suppressRows(payload.flows, receipt, "architecture_flow");
          return { ...payload, flows: filtered.rows, omissions: filtered.omissions };
        }
        if (view === "liveness") {
          return { ...buildLivenessProjection(loadGeneration(db), {
            sourceState: receipt.freshness === "fresh" ? "clean" : "stale",
            maxNodes: input.maxNodes,
            maxEdges: input.maxEdges,
            maxHops: input.maxHops,
          }), freshnessReceipt: receipt };
        }
        if (view === "processes") {
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
          const generation = loadGeneration(db);
          const projection = buildDisposableArchitectureProjection({
            nodes: generation.nodes,
            edges: generation.edges,
            generationId: meta.manifest.generationId,
            maxComponents: Number(input.maxComponents ?? 100),
            maxFlows: Number(input.maxFlows ?? 200),
          });
          const components = suppressRows(projection.components, receipt, "architecture_component");
          const allowed = new Set(components.rows.map((component) => component.id));
          const flows = suppressRows(projection.flows.filter((flow) => allowed.has(flow.from) && allowed.has(flow.to)), receipt, "architecture_flow");
          return { ...projection, components: components.rows, flows: flows.rows, omissions: [...projection.omissions, ...components.omissions, ...flows.omissions], freshnessReceipt: receipt };
        }
        if (view === "changes") {
          return { ...changesSinceReference(db, root, {
            snapshot: input.snapshot,
            generation: input.sinceGeneration,
            treeish: input.treeish,
            head: input.head,
            limit: input.limit,
          }), freshnessReceipt: receipt };
        }
        if (view !== "summary") fail("architecture_view_invalid", "Architecture view must be summary, flows, liveness, processes, contracts, signatures, orientation, projection, or changes.");
        return suppressTraversalPayload({ ...boundedArchitecture(db, {
          budget: Number(input.budget ?? 2000),
          cursor: input.cursor,
          freshness,
        }), view: "summary" }, receipt);
      }, options);
    },

    async federate(input = {}, options = {}) {
      const group = input.group ?? null;
      const repositories = group?.repositories ?? input.repositories ?? [];
      const operation = String(input.operation ?? "recall");
      const allowedRepoIds = input.allowedRepoIds ?? repositories.map((repository) => repository.repoId);
      return routeFederatedQuery({
        group,
        repositories,
        allowedRepoIds,
        operation,
        input: input.query ?? {},
        querySlice: async (repository, method, queryInput) => {
          const child = createBlueprintApplicationService({
            outDir,
            rootRegistry,
            allowEmbeddedRoot,
            freshnessOwnership,
            buildSingleflight,
            liveVerifier,
          });
          const result = await child[method]({
            ...queryInput,
            repoId: repository.repoId,
            repoRoot: repository.repoRoot,
            generation: repository.generation,
          }, options);
          return { ...result, repoRoot: child.resolveCanonicalRoot(repository) };
        },
      });
    },

    async documentTruth(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const claims = listClaimSlice(db, meta.manifest.generationId, {
          limit: Number(input.limit ?? 200),
          claimId: input.claimId,
          kind: input.kind,
        });
        const supersedes = listDocumentSupersession(db, meta.manifest.generationId, { limit: Number(input.limit ?? 200) });
        const grounding = projectDocumentTruth({
          claims,
          supersedes,
          generationId: meta.manifest.generationId,
          freshness: receipt.freshness,
        });
        return {
          schemaVersion: 1,
          generationId: meta.manifest.generationId,
          claims,
          grounding: grounding.claims,
          groundingCounts: grounding.counts,
          supersedes: grounding.supersedes,
          freshnessReceipt: receipt,
          omissions: [],
          truncated: false,
        };
      }, options);
    },
  });
}
