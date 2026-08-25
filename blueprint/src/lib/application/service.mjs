import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { observeCurrentSourceAtPath, syncToCurrentSourceAtPath } from "../../graph/barrier.mjs";
import {
  closeStore,
  listClaimSlice,
  openStoreReadOnly,
} from "../../graph/store-sqlite.mjs";
import {
  boundedArchitecture,
  boundedImpact,
  boundedNeighbors,
  indexedQueryGeneration,
  indexedResolve,
  readIndexedMeta,
} from "../../graph/traverse-store.mjs";
import {
  graphStatus,
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

    async architecture(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, receipt }) => boundedArchitecture(db, {
        budget: Number(input.budget ?? 2000),
        cursor: input.cursor,
        freshness: {
          generationId: receipt.generationId,
          sourceState: receipt.barrierResult === "caught_up" ? "clean" : "stale",
          dirtyFileCount: 0,
        },
      }), options);
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
