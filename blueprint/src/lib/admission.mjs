// Blueprint admission — decision-only library.
//
// Operations: recall / expand / status / revoke.
// Returns a neutral decision contract Cortex (or any host) can consume.
// Does NOT install hooks, classify shell, or fail-closed on tool use.

import { createHash } from "node:crypto";
import { basename, dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  buildOrientationReceipt,
  createReceiptStore,
  receiptLookupKey,
} from "./receipt-store.mjs";
import {
  buildOrientationEvidence,
  candidateSetDigest,
  writeOrientationEvidenceFile,
  defaultEvidencePath,
} from "./orientation-evidence.mjs";

export const ADMISSION_SCHEMA_VERSION = 1;
export const DECISION_ACTIONS = Object.freeze(["allow", "continue", "block", "noop"]);

const DEFAULT_OPERATIONS = Object.freeze(["read", "search", "test", "edit"]);

function decision(partial) {
  const action = partial.action ?? "noop";
  if (!DECISION_ACTIONS.includes(action)) {
    throw new Error(`invalid admission action: ${action}`);
  }
  return {
    schemaVersion: ADMISSION_SCHEMA_VERSION,
    action,
    reason: String(partial.reason ?? ""),
    reasonCode: partial.reasonCode ?? null,
    receiptId: partial.receiptId ?? null,
    candidateSet: partial.candidateSet ?? null,
    allowedScopes: [...(partial.allowedScopes ?? [])],
    omissions: [...(partial.omissions ?? [])],
    claimBoundary: partial.claimBoundary ?? genericClaimBoundary(),
    nextAction: partial.nextAction ?? null,
    evidence: partial.evidence ?? null,
    evidencePath: partial.evidencePath ?? null,
    receipt: partial.receipt ?? null,
  };
}

function genericClaimBoundary() {
  return claimBoundaryFor({ permitClean: false, state: "missing", generationId: null, omissions: [] });
}

function claimBoundaryFor({ permitClean, state, generationId, omissions }) {
  const restricted = !permitClean;
  const fresh = state === "fresh";
  const claimRestricted = restricted || !fresh;
  const gaps = [...(omissions ?? [])]
    .map((omission) => String(omission?.reason ?? ""))
    .filter(Boolean);
  return {
    status: claimRestricted ? "restricted" : "clear",
    cleanClaimAllowed: !claimRestricted,
    safeClaims: fresh
      ? ["Results reflect the current sealed generation."]
      : generationId
        ? [`Results reflect generation ${generationId}, which predates current worktree changes.`]
        : ["No sealed graph generation is available to ground claims."],
    prohibitedClaims: claimRestricted ? ["Graph-derived facts are current."] : [],
    gaps,
  };
}

function normalizeRepoPath(value) {
  return String(value ?? "").replace(/\\/g, "/").replace(/^\.\//, "").replace(/\/+$/, "");
}

function isAbsoluteInput(value) {
  return isAbsolute(value) || /^[A-Za-z]:[\\/]/.test(value);
}

function directoryScope(filePath) {
  const normalized = normalizeRepoPath(filePath);
  const dir = dirname(normalized);
  return dir === "." ? "" : dir;
}

function pathsFromCandidateSet(candidateSet) {
  const paths = new Set();
  for (const candidate of candidateSet?.candidates ?? []) {
    const ref = String(candidate.sourceRef ?? "");
    const path = ref.includes(":") ? ref.slice(0, ref.lastIndexOf(":")) : ref;
    const normalized = normalizeRepoPath(path);
    if (normalized) paths.add(normalized);
  }
  return [...paths].sort();
}

function scopesFromPaths(paths) {
  const scopes = new Set();
  for (const path of paths) {
    const dir = directoryScope(path);
    if (dir) scopes.add(dir);
  }
  return [...scopes].sort();
}

function turnDigest(task, sessionId) {
  const payload = `${sessionId ?? ""}\n${task ?? ""}`;
  return `sha256:${createHash("sha256").update(payload).digest("hex")}`;
}

function repoIdentityFromRoot(repoRoot) {
  const root = resolve(repoRoot);
  return `${basename(root)}@${root}`;
}

async function defaultDeps() {
  const staticProvider = await import("../graph/static-provider.mjs");
  return {
    readGeneration: (repoRoot, outDir) => staticProvider.readGeneration(repoRoot, outDir),
    createContextCandidateSet: (generation, options) =>
      staticProvider.createContextCandidateSet(generation, options),
    graphStatus: (repoRoot, outDir, options) =>
      staticProvider.graphStatus(repoRoot, outDir, options),
  };
}

/**
 * Create an admission API bound to a receipt store and optional graph deps.
 *
 * @param {{
 *   store?: ReturnType<typeof createReceiptStore>,
 *   storeDir?: string,
 *   outDir?: string,
 *   readGeneration?: Function,
 *   createContextCandidateSet?: Function,
 *   graphStatus?: Function,
 *   evidenceDir?: string,
 * }} [options]
 */
export function createAdmission(options = {}) {
  const store = options.store ?? createReceiptStore({ storeDir: options.storeDir });
  const outDir = options.outDir ?? ".agent";
  const evidenceDir = options.evidenceDir ?? null;
  let depsPromise = null;

  async function deps() {
    if (options.readGeneration && options.createContextCandidateSet) {
      return {
        readGeneration: options.readGeneration,
        createContextCandidateSet: options.createContextCandidateSet,
        graphStatus: options.graphStatus ?? (() => ({ state: "fresh" })),
      };
    }
    if (!depsPromise) depsPromise = defaultDeps();
    const loaded = await depsPromise;
    return {
      readGeneration: options.readGeneration ?? loaded.readGeneration,
      createContextCandidateSet: options.createContextCandidateSet ?? loaded.createContextCandidateSet,
      graphStatus: options.graphStatus ?? loaded.graphStatus,
    };
  }

  function attachEvidence(receipt, extra = {}) {
    if (!receipt) return { evidence: null, evidencePath: null };
    const evidence = buildOrientationEvidence(receipt);
    let evidencePath = null;
    if (evidenceDir) {
      const written = writeOrientationEvidenceFile(
        receipt,
        defaultEvidencePath(evidenceDir, receipt.receiptId),
      );
      evidencePath = written.path;
    }
    return { evidence, evidencePath, ...extra };
  }

  /**
   * Establish task-scoped recall. Decision-only — does not block hosts.
   */
  async function recall(input = {}) {
    const task = String(input.task ?? input.query ?? "").trim();
    const sessionId = String(input.sessionId ?? "default");
    const taskId = String(input.taskId ?? (task || "untasked"));
    const repoRoot = resolve(input.repoRoot ?? process.cwd());
    const repoIdentity = String(input.repoIdentity ?? repoIdentityFromRoot(repoRoot));
    const anchors = (input.anchors ?? input.explicitAnchors ?? []).map(normalizeRepoPath);
    const { readGeneration, createContextCandidateSet, graphStatus } = await deps();

    const status = graphStatus(repoRoot, outDir, input.statusOptions ?? {});
    if (!status || status.state === "missing" || status.state === "incomplete") {
      return decision({
        action: "block",
        reason: "No complete Blueprint graph generation is available for recall.",
        reasonCode: "missing_graph",
        nextAction: `blueprint build --out ${outDir}`,
        omissions: [{ reason: "missing_graph" }],
        claimBoundary: claimBoundaryFor({
          permitClean: false,
          state: status?.state ?? "missing",
          generationId: null,
          omissions: [{ reason: "missing_graph" }],
        }),
      });
    }

    const generation = readGeneration(repoRoot, outDir);
    if (!generation?.manifest?.generationId) {
      return decision({
        action: "block",
        reason: "Graph store could not load a sealed generation for recall.",
        reasonCode: "missing_generation",
        nextAction: `blueprint build --out ${outDir}`,
        claimBoundary: claimBoundaryFor({
          permitClean: false,
          state: status.state,
          generationId: null,
          omissions: [{ reason: "missing_generation" }],
        }),
      });
    }

    const generationId = generation.manifest.generationId;
    const manifestDigest = generation.manifest.manifestDigest ?? null;

    if (input.expectedGeneration && input.expectedGeneration !== generationId) {
      return decision({
        action: "block",
        reason: `Pinned generation ${input.expectedGeneration} does not match live ${generationId}.`,
        reasonCode: "generation_mismatch",
        nextAction: "Re-run recall against the current generation, or rebuild the graph.",
        claimBoundary: claimBoundaryFor({
          permitClean: false,
          state: status.state,
          generationId,
          omissions: [],
        }),
      });
    }

    const existing = store.findActive({ sessionId, taskId, repoIdentity, generationId });
    if (existing && !input.force) {
      const { evidence, evidencePath } = attachEvidence(existing);
      return decision({
        action: "continue",
        reason: "Active recall receipt already exists for this session/task/repo/generation.",
        reasonCode: "receipt_reuse",
        receiptId: existing.receiptId,
        allowedScopes: existing.allowedPaths ?? [],
        omissions: existing.omissions ?? [],
        nextAction: null,
        evidence,
        evidencePath,
        receipt: existing,
        candidateSet: null,
        claimBoundary: claimBoundaryFor({
          permitClean: false,
          state: status.state,
          generationId,
          omissions: existing.omissions ?? [],
        }),
      });
    }

    const candidateSet = createContextCandidateSet(generation, {
      task,
      query: input.query ?? task,
      anchors,
      maxCandidates: input.maxCandidates,
      maxEstimatedTokens: input.maxEstimatedTokens,
      traceId: input.traceId,
      mode: input.mode ?? "survey",
      repoId: input.repoId,
      repoRoot,
      receiptId: input.freshnessReceipt?.receiptId ?? input.receiptId ?? null,
    });
    if (input.freshnessReceipt) { candidateSet.freshnessReceipt = input.freshnessReceipt; candidateSet.receiptId = input.freshnessReceipt.receiptId; }
    if (input.neighborhood) candidateSet.neighborhood = input.neighborhood;

    const allowedPaths = [...new Set([...anchors, ...pathsFromCandidateSet(candidateSet)])].sort();
    const allowedDirectoryScopes = scopesFromPaths(allowedPaths);
    const digest = candidateSetDigest(candidateSet);
    const receipt = buildOrientationReceipt({
      receiptId: store.nextId(),
      sessionId,
      taskId,
      turnDigest: input.turnDigest ?? turnDigest(task, sessionId),
      repoIdentity,
      worktreeIdentity: input.worktreeIdentity ?? pathToFileURL(repoRoot).href,
      generationId,
      manifestDigest,
      sourceObservationDigest: generation.sourceObservation
        ? `sha256:${createHash("sha256").update(JSON.stringify(generation.sourceObservation)).digest("hex")}`
        : null,
      candidateSetDigest: digest,
      explicitAnchors: anchors,
      allowedPaths,
      allowedDirectoryScopes,
      allowedOperations: input.allowedOperations ?? [...DEFAULT_OPERATIONS],
      overlayRevision: Number(status.capabilities?.dirtyOverlayFileCount > 0 ? 1 : 0),
      omissions: candidateSet.omissions ?? [],
    });
    store.put(receipt);
    const { evidence, evidencePath } = attachEvidence(receipt);

    const action = status.state === "stale" || status.state === "indeterminate" ? "continue" : "allow";
    return decision({
      action,
      reason: action === "allow"
        ? "Recall established against the sealed graph generation."
        : `Recall established under ${status.state} graph state; overlay/freshness is explicit on the receipt.`,
      reasonCode: action === "allow" ? "recalled" : `recalled_${status.state}`,
      receiptId: receipt.receiptId,
      candidateSet,
      allowedScopes: allowedPaths,
      omissions: candidateSet.omissions ?? [],
      nextAction: status.state === "stale" ? `blueprint build --out ${outDir}` : null,
      evidence,
      evidencePath,
      receipt,
      claimBoundary: claimBoundaryFor({
        permitClean: action === "allow",
        state: status.state,
        generationId,
        omissions: candidateSet.omissions ?? [],
      }),
    });
  }

  /**
   * Expand an existing receipt with graph-supported paths for a path/symbol/query.
   */
  async function expand(input = {}) {
    const receiptId = input.receiptId;
    if (!receiptId) {
      return decision({
        action: "block",
        reason: "expand requires receiptId.",
        reasonCode: "missing_receipt_id",
        nextAction: "Call recall first, then expand with the returned receiptId.",
      });
    }
    const receipt = store.get(receiptId);
    if (!receipt) {
      return decision({
        action: "block",
        reason: `No receipt found for ${receiptId}.`,
        reasonCode: "receipt_not_found",
        nextAction: "Call recall to establish a receipt.",
      });
    }
    if (receipt.status === "revoked") {
      return decision({
        action: "block",
        reason: "Receipt is revoked; recall again before expanding.",
        reasonCode: "receipt_revoked",
        receiptId,
        nextAction: "Call recall with force=true or a new session/task.",
      });
    }

    const query = String(input.query ?? input.path ?? input.symbol ?? input.task ?? "").trim();
    const relativePaths = (input.paths ?? []).map((p) => String(p));

    // Models cannot self-approve arbitrary absolute scopes — only relative repo paths / queries.
    for (const p of relativePaths) {
      if (isAbsoluteInput(p)) {
        return decision({
          action: "block",
          reason: "expand rejects absolute self-approved paths; pass a relative path or graph query.",
          reasonCode: "absolute_path_rejected",
          receiptId,
        });
      }
    }
    if (input.path) {
      const p = String(input.path);
      if (isAbsoluteInput(p)) {
        return decision({
          action: "block",
          reason: "expand rejects absolute self-approved paths; pass a relative path or graph query.",
          reasonCode: "absolute_path_rejected",
          receiptId,
        });
      }
    }

    if (!query && relativePaths.length === 0) {
      return decision({
        action: "block",
        reason: "expand requires a path, symbol, or query grounded in the graph.",
        reasonCode: "missing_expand_query",
        receiptId,
        nextAction: "Pass path, symbol, or query.",
      });
    }

    const identityPath = receipt.repoIdentity.includes("@")
      ? receipt.repoIdentity.slice(receipt.repoIdentity.indexOf("@") + 1)
      : "";
    const repoRoot = resolve(input.repoRoot || identityPath || process.cwd());
    const { readGeneration, createContextCandidateSet } = await deps();
    const generation = readGeneration(repoRoot, outDir);
    if (!generation?.manifest?.generationId) {
      return decision({
        action: "block",
        reason: "Graph generation missing; cannot expand from evidence.",
        reasonCode: "missing_generation",
        receiptId,
        nextAction: `blueprint build --out ${outDir}`,
      });
    }
    if (receipt.generationId && receipt.generationId !== generation.manifest.generationId) {
      return decision({
        action: "block",
        reason: "Graph generation changed after recall; recall again before expand.",
        reasonCode: "generation_changed",
        receiptId,
        nextAction: "Call recall against the current generation.",
      });
    }

    const expandQuery = query || relativePaths[0] || "";
    const anchors = [
      ...receipt.explicitAnchors,
      ...relativePaths.map(normalizeRepoPath),
      ...(input.path ? [normalizeRepoPath(input.path)] : []),
    ];
    const candidateSet = createContextCandidateSet(generation, {
      task: expandQuery,
      query: expandQuery,
      anchors,
      maxCandidates: input.maxCandidates,
      maxEstimatedTokens: input.maxEstimatedTokens,
      mode: input.mode ?? "survey",
      repoId: input.repoId,
      repoRoot,
      receiptId: input.freshnessReceipt?.receiptId ?? input.receiptId ?? receipt.receiptId,
    });
    if (input.freshnessReceipt) { candidateSet.freshnessReceipt = input.freshnessReceipt; candidateSet.receiptId = input.freshnessReceipt.receiptId; }
    if (input.neighborhood) candidateSet.neighborhood = input.neighborhood;
    const addedPaths = pathsFromCandidateSet(candidateSet);
    const allowedPaths = [...new Set([...(receipt.allowedPaths ?? []), ...anchors, ...addedPaths])].sort();
    const updated = {
      ...receipt,
      explicitAnchors: [...new Set([...(receipt.explicitAnchors ?? []), ...anchors])].sort(),
      allowedPaths,
      allowedDirectoryScopes: scopesFromPaths(allowedPaths),
      candidateSetDigest: candidateSetDigest(candidateSet),
      omissions: [...(receipt.omissions ?? []), ...(candidateSet.omissions ?? [])],
      overlayRevision: Number(receipt.overlayRevision ?? 0) + 1,
      updatedAt: new Date().toISOString(),
    };
    store.put(updated);
    const { evidence, evidencePath } = attachEvidence(updated);

    return decision({
      action: "continue",
      reason: `Expanded recall with ${addedPaths.length} graph-supported path(s).`,
      reasonCode: "expanded",
      receiptId: updated.receiptId,
      candidateSet,
      allowedScopes: allowedPaths,
      omissions: candidateSet.omissions ?? [],
      nextAction: null,
      evidence,
      evidencePath,
      receipt: updated,
    });
  }

  /**
   * Inspect the current receipt / graph recall state.
   */
  async function status(input = {}) {
    let receipt = null;
    let graphState = null;
    if (input.receiptId) {
      receipt = store.get(input.receiptId);
    } else if (input.sessionId != null || input.taskId != null || input.repoIdentity || input.generationId) {
      receipt = store.findActive({
        sessionId: input.sessionId ?? "default",
        taskId: input.taskId ?? input.task ?? "",
        repoIdentity: input.repoIdentity ?? (input.repoRoot ? repoIdentityFromRoot(input.repoRoot) : ""),
        generationId: input.generationId ?? "",
      });
      // Soft lookup: if generationId omitted, scan active receipts for session/task/repo.
      if (!receipt && !input.generationId) {
        const needle = receiptLookupKey({
          sessionId: input.sessionId ?? "default",
          taskId: input.taskId ?? input.task ?? "",
          repoIdentity: input.repoIdentity ?? (input.repoRoot ? repoIdentityFromRoot(input.repoRoot) : ""),
          generationId: "",
        }).replace(/\u001f$/, "");
        receipt = store.list().reverse().find((row) => {
          const key = receiptLookupKey(row);
          return key.startsWith(needle) && row.status !== "revoked";
        }) ?? null;
      }
    }

    if (!receipt) {
      return decision({
        action: "noop",
        reason: "No recall receipt found.",
        reasonCode: "no_receipt",
        nextAction: "Call recall for the active session/task/repo.",
        claimBoundary: claimBoundaryFor({
          permitClean: false,
          state: "missing",
          generationId: null,
          omissions: [{ reason: "no_receipt" }],
        }),
      });
    }

    if (receipt.status === "revoked") {
      return decision({
        action: "block",
        reason: "Recall receipt is revoked.",
        reasonCode: "receipt_revoked",
        receiptId: receipt.receiptId,
        receipt,
        nextAction: "Call recall to establish a new receipt.",
        claimBoundary: claimBoundaryFor({
          permitClean: false,
          state: "revoked",
          generationId: receipt.generationId ?? null,
          omissions: receipt.omissions ?? [],
        }),
      });
    }

    const { evidence, evidencePath } = attachEvidence(receipt);
    const worktreeIdentity = receipt.worktreeIdentity ?? null;
    const repoRoot = resolve(
      input.repoRoot
        ?? (worktreeIdentity && String(worktreeIdentity).startsWith("file:")
          ? fileURLToPath(worktreeIdentity)
          : process.cwd()),
    );
    const { graphStatus } = await deps();
    graphState = graphStatus(repoRoot, outDir, input.statusOptions ?? {})?.state ?? "missing";
    return decision({
      action: "continue",
      reason: "Active recall receipt.",
      reasonCode: "receipt_active",
      receiptId: receipt.receiptId,
      allowedScopes: receipt.allowedPaths ?? [],
      omissions: receipt.omissions ?? [],
      evidence,
      evidencePath,
      receipt,
      claimBoundary: claimBoundaryFor({
        permitClean: true,
        state: graphState,
        generationId: receipt.generationId ?? null,
        omissions: receipt.omissions ?? [],
      }),
    });
  }

  /**
   * Explicitly invalidate a receipt.
   */
  async function revoke(input = {}) {
    const receiptId = input.receiptId;
    if (!receiptId) {
      return decision({
        action: "block",
        reason: "revoke requires receiptId.",
        reasonCode: "missing_receipt_id",
      });
    }
    const existing = store.get(receiptId);
    if (!existing) {
      return decision({
        action: "noop",
        reason: `No receipt found for ${receiptId}.`,
        reasonCode: "receipt_not_found",
        receiptId,
      });
    }
    if (existing.status === "revoked") {
      const { evidence } = attachEvidence(existing);
      return decision({
        action: "noop",
        reason: "Receipt already revoked.",
        reasonCode: "already_revoked",
        receiptId,
        evidence,
        receipt: existing,
      });
    }
    const revoked = store.revoke(receiptId, { reason: input.reason ?? "explicit_revoke" });
    const { evidence, evidencePath } = attachEvidence(revoked);
    return decision({
      action: "allow",
      reason: "Recall receipt revoked.",
      reasonCode: "revoked",
      receiptId,
      evidence,
      evidencePath,
      receipt: revoked,
      nextAction: "Call recall before further repository recall-dependent work.",
    });
  }

  return {
    store,
    recall,
    expand,
    status,
    revoke,
    decision,
  };
}

/** Convenience singleton factory using default host store. */
export function recall(input, options) {
  return createAdmission(options).recall(input);
}
export function expand(input, options) {
  return createAdmission(options).expand(input);
}
export function status(input, options) {
  return createAdmission(options).status(input);
}
export function revoke(input, options) {
  return createAdmission(options).revoke(input);
}

export {
  buildOrientationEvidence,
  candidateSetDigest,
  writeOrientationEvidenceFile,
  createReceiptStore,
  buildOrientationReceipt,
};
