import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { observeCurrentSourceAtPath, syncToCurrentSourceAtPath } from "../../graph/barrier.mjs";
import {
  closeStore,
  listClaimSlice,
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
import { observeRepositoryFreshness } from "../../sources/freshness-observation.mjs";
import { serviceStatus } from "../../service/status.mjs";
import { fail } from "./errors.mjs";

function databasePath(root, outDir) {
  return join(root, outDir, "graph", "graph.db");
}

function throwIfAborted(signal) {
  if (signal?.aborted) fail("request_cancelled", "request cancelled");
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
  allowEmbeddedRoot = true,
  freshnessOwnership = "one_shot",
} = {}) {
  if (!["one_shot", "resident"].includes(freshnessOwnership)) {
    throw new TypeError("freshnessOwnership must be one_shot or resident");
  }
  const resolveRoot = (input = {}) => {
    if (rootRegistry) return rootRegistry.resolve(input);
    if (!allowEmbeddedRoot) fail("root_not_enrolled", "No enrolled Blueprint repository matches this request.");
    return resolve(input.repoRoot ?? process.cwd());
  };

  async function openFreshnessSession(input = {}, { signal } = {}) {
    throwIfAborted(signal);
    const root = resolveRoot(input);
    const dbPath = databasePath(root, outDir);
    if (!existsSync(dbPath)) fail("graph_missing", `Graph store is missing for ${root}.`);
    let receipt;
    try {
      receipt = freshnessOwnership === "resident"
        ? observeCurrentSourceAtPath(root, { outDir })
        : await syncToCurrentSourceAtPath(root, { outDir, timeoutMs: Number(input.timeoutMs ?? 2000), signal });
    } catch (error) {
      if (error?.code === "request_cancelled") fail("request_cancelled", "request cancelled");
      throw error;
    }
    throwIfAborted(signal);
    const db = openStoreReadOnly(dbPath);
    try {
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
      let closed = false;
      return Object.freeze({
        root,
        db,
        meta,
        receipt: freshnessReceipt,
        close() {
          if (closed) return;
          closed = true;
          closeStore(db);
        },
      });
    } catch (error) {
      closeStore(db);
      throw error;
    }
  }

  async function withCurrentDb(input, callback, { signal, session = null } = {}) {
    const current = session ?? await openFreshnessSession(input, { signal });
    try {
      throwIfAborted(signal);
      const result = await callback(current);
      throwIfAborted(signal);
      return result;
    } finally {
      if (!session) current.close();
    }
  }

  async function resolveAnchor(input, options) {
    if (String(input.anchor ?? "").startsWith("file:") || String(input.anchor ?? "").startsWith("symbol:")) {
      return String(input.anchor);
    }
    return withCurrentDb(input, ({ db }) => {
      const generation = indexedQueryGeneration(db, String(input.anchor ?? ""), { limit: 8 });
      const matches = queryGraph(generation, { query: String(input.anchor ?? ""), limit: 8 });
      const exact = matches.filter((match) => match.id === input.anchor || match.name === input.anchor || match.qualifiedName === input.anchor || match.path === input.anchor);
      if (exact.length === 1) return exact[0].id;
      if (exact.length === 0 && matches.length === 1) return matches[0].id;
      if (exact.length === 0) fail("anchor_not_found", `No graph anchor matches ${input.anchor}.`);
      fail("anchor_ambiguous", `More than one graph anchor matches ${input.anchor}.`, { candidates: exact.map((x) => x.id) });
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
        const generation = indexedQueryGeneration(db, query, { limit: Number(input.limit ?? 20), anchors: input.anchors ?? [] });
        return {
          schemaVersion: 1,
          kind: "search",
          generationId: meta.manifest.generationId,
          provider: meta.provider,
          query,
          results: queryGraph(generation, { query, limit: Number(input.limit ?? 20) }),
          omissions: [],
          truncated: false,
          continuationCursor: null,
          freshnessReceipt: receipt,
        };
      }, options);
    },

    async resolve(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const result = indexedResolve(db, String(input.nodeId ?? ""), {
          sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
        });
        if (!result) fail("node_not_found", `Graph node not found: ${input.nodeId}`);
        return { ...result, generationId: meta.manifest.generationId, freshnessReceipt: receipt };
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
          canonical: true,
        });
        return {
          schemaVersion: 1,
          action: "allow",
          reasonCode: "recalled",
          generationId: meta.manifest.generationId,
          candidateSet: candidates,
          recallCircuit: circuit,
          freshnessReceipt: receipt,
          omissions: candidates.omissions ?? [],
        };
      }, options);
    },

    async expand(input = {}, options = {}) {
      const session = await openFreshnessSession(input, options);
      try {
        const scopedOptions = { ...options, session };
        const nodeId = await resolveAnchor(input, scopedOptions);
        return withCurrentDb(input, ({ db, receipt }) => boundedNeighbors(db, {
          nodeId,
          direction: input.direction ?? "both",
          depth: Number(input.depth ?? 1),
          budget: Number(input.budget ?? 2000),
          cursor: input.cursor,
          freshness: {
            generationId: receipt.generationId,
            sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
            dirtyFileCount: 0,
          },
        }), scopedOptions);
      } finally {
        session.close();
      }
    },

    async impact(input = {}, options = {}) {
      const session = await openFreshnessSession(input, options);
      try {
        const scopedOptions = { ...options, session };
        const nodeId = await resolveAnchor(input, scopedOptions);
        return withCurrentDb(input, ({ db, receipt }) => boundedImpact(db, {
          nodeId,
          depth: Number(input.depth ?? 3),
          budget: Number(input.budget ?? 2000),
          cursor: input.cursor,
          freshness: {
            generationId: receipt.generationId,
            sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
            dirtyFileCount: 0,
          },
        }), scopedOptions);
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
        return withCurrentDb(input, ({ db, receipt }) => boundedPath(db, {
          from,
          to,
          maxDepth,
          budget,
          cursor: input.cursor,
          freshness: {
            generationId: receipt.generationId,
            sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
            dirtyFileCount: 0,
          },
        }), scopedOptions);
      } finally {
        session.close();
      }
    },

    async architecture(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const freshness = {
          generationId: receipt.generationId,
          sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
          dirtyFileCount: 0,
        };
        const view = input.view ?? "summary";
        if (view === "flows") return architectureFlowPage(db, meta, input, freshness);
        if (view !== "summary") fail("architecture_view_invalid", "Architecture view must be summary or flows.");
        return { ...boundedArchitecture(db, {
          budget: Number(input.budget ?? 2000),
          cursor: input.cursor,
          freshness,
        }), view: "summary" };
      }, options);
    },

    async documentTruth(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => ({
        schemaVersion: 1,
        generationId: meta.manifest.generationId,
        claims: listClaimSlice(db, meta.manifest.generationId, {
          limit: Number(input.limit ?? 200),
          claimId: input.claimId,
          kind: input.kind,
        }),
        freshnessReceipt: receipt,
        omissions: [],
        truncated: false,
      }), options);
    },
  });
}
