import { createXXHash128 } from "hash-wasm";

export const PHASE2_INCREMENTAL_VERSION = 1;
export const DIMENSIONS = Object.freeze([
  "architecture",
  "interfaces",
  "health",
  "contract",
  "security",
  "solid",
]);

const VERIFIABLE_STATUSES = new Set([
  "implemented",
  "stale",
  "contradict",
  "decision",
  "canonical",
]);

function stableValue(value) {
  if (Array.isArray(value)) return value.map(stableValue);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.keys(value).sort().map((key) => [key, stableValue(value[key])]),
  );
}

// XXH3-128 (via hash-wasm) — content/change-detection fingerprints for Phase 2
// verdict and dimension reuse. No trust boundary depends on collision
// resistance: the Blueprint→Crypt federation provider reads only the
// generation identity, never validates it cryptographically
// (membrane/engine/federation/providers/blueprint.py). hash-wasm's own
// constructor is async (WASM init), but init()/update()/digest() are
// synchronous once it exists, so the async cost is paid exactly once at
// module load (top-level await) and every call site below stays synchronous
// — no per-call Promise, no blocking busy-wait.
const xxhasher = await createXXHash128();

function xxh128(value) {
  xxhasher.init();
  xxhasher.update(value);
  return xxhasher.digest("hex");
}

function fingerprint(value) {
  return `xxh128:${xxh128(JSON.stringify(stableValue(value)))}`;
}

function uniqueSorted(values) {
  return [...new Set((values ?? []).map(String).filter(Boolean))].sort();
}

export function graphFileHashes(graph) {
  const hashes = new Map();
  for (const node of graph?.nodes ?? []) {
    if (node?.kind !== "file") continue;
    const evidence = (node.evidence ?? []).find((item) => item?.path && item?.contentHash);
    const path = evidence?.path ?? node.path;
    if (path && evidence?.contentHash) hashes.set(path, evidence.contentHash);
  }
  return hashes;
}

function eligibleClaims(queue) {
  return (queue?.claims ?? []).filter((claim) =>
    VERIFIABLE_STATUSES.has(claim?.status) || (claim?.candidateFiles?.length ?? 0) > 0,
  );
}

function claimDescriptor(claim, hashes) {
  const inputFiles = uniqueSorted([claim.source, ...(claim.candidateFiles ?? [])]);
  const inputs = inputFiles.map((path) => ({ path, contentHash: hashes.get(path) ?? null }));
  const missingInputs = inputs.filter((item) => !item.contentHash).map((item) => item.path);
  return {
    claimId: claim.id,
    inputFiles,
    inputFingerprint: fingerprint({
      schemaVersion: PHASE2_INCREMENTAL_VERSION,
      claim: {
        id: claim.id,
        source: claim.source,
        line: claim.line,
        status: claim.status,
        text: claim.text,
      },
      inputs,
    }),
    missingInputs,
  };
}

export function claimEvidenceFingerprint(claim, graph) {
  return claimDescriptor(claim, graphFileHashes(graph)).inputFingerprint;
}

function semanticVerdict(verdict) {
  if (!verdict || typeof verdict !== "object") return null;
  const {
    inputFiles: _inputFiles,
    inputFingerprint: _inputFingerprint,
    reconciliationFingerprint: _reconciliationFingerprint,
    sourceGenerationId: _sourceGenerationId,
    ...semantic
  } = verdict;
  return semantic;
}

export function verdictResultFingerprint(verdict) {
  return fingerprint({
    schemaVersion: PHASE2_INCREMENTAL_VERSION,
    verdict: semanticVerdict(verdict),
  });
}

function reconciliationFingerprintPayload(verdict) {
  return {
    schemaVersion: PHASE2_INCREMENTAL_VERSION,
    kind: "reconciliation",
    verdict: semanticVerdict(verdict),
  };
}

export function reconcileInputFingerprint(verdict) {
  return fingerprint(reconciliationFingerprintPayload(verdict));
}

// Case (c): this gates a STORED USER DECISION, not a content-cache reuse.
// A Phase-4 reconciliation decision is "current" while its recorded
// fingerprint still describes today's verdict.
//
// A legacy sha256: branch used to live here, to avoid re-opening decisions
// sealed before the xxh3 migration. It was removed 2026-07-26 after measuring
// what it actually protected: across every .agent/reconcile.json on disk there
// were 11 entries and ZERO with a non-null `decision`. The guard clause below
// already rejects those, so the legacy branch was unreachable for every
// artifact that exists — it bridged a transition during which nobody recorded
// a decision. A stale sha256-fingerprinted entry now simply re-verifies on the
// next run, which costs a recompute and loses no human judgement.
export function isReconciliationDecisionCurrent(verdict, decision) {
  if (decision?.decision == null) return false;
  const stored = decision?.inputFingerprint;
  if (typeof stored !== "string") return false;
  return stored === reconcileInputFingerprint(verdict);
}

function dimensionDescriptor(dimension, metadata, hashes, verdictsById) {
  const inputFiles = uniqueSorted(metadata?.inputFiles);
  const inputVerdictIds = uniqueSorted(metadata?.inputVerdictIds);
  const files = inputFiles.map((path) => ({ path, contentHash: hashes.get(path) ?? null }));
  const verdicts = inputVerdictIds.map((claimId) => ({
    claimId,
    resultFingerprint: verdictsById.has(claimId)
      ? verdictResultFingerprint(verdictsById.get(claimId))
      : null,
  }));
  return {
    inputFiles,
    inputVerdictIds,
    missingFiles: files.filter((item) => !item.contentHash).map((item) => item.path),
    missingVerdicts: verdicts.filter((item) => !item.resultFingerprint).map((item) => item.claimId),
    fileInventoryFingerprint: fingerprint({
      schemaVersion: PHASE2_INCREMENTAL_VERSION,
      kind: "repository-file-inventory",
      paths: [...hashes.keys()].sort(),
    }),
    inputFingerprint: fingerprint({
      schemaVersion: PHASE2_INCREMENTAL_VERSION,
      dimension,
      files,
      verdicts,
    }),
  };
}

export function buildIncrementalPhase2Plan({ graph, queue, verdictEnvelope, understanding }) {
  const sourceGenerationId = graph?.manifest?.generationId ?? null;
  const hashes = graphFileHashes(graph);
  const previousVerdicts = new Map(
    (verdictEnvelope?.verdicts ?? []).map((item) => [item.claimId ?? item.id, item]),
  );
  const reusedVerdicts = new Map();
  const reuse = [];
  const verify = [];

  for (const claim of eligibleClaims(queue)) {
    const descriptor = claimDescriptor(claim, hashes);
    const previous = previousVerdicts.get(claim.id);
    if (previous && previous.inputFingerprint === descriptor.inputFingerprint && !descriptor.missingInputs.length) {
      const reused = { ...previous, inputFiles: descriptor.inputFiles, inputFingerprint: descriptor.inputFingerprint };
      reusedVerdicts.set(claim.id, reused);
      reuse.push(reused);
    } else {
      verify.push({
        ...claim,
        ...descriptor,
        reason: descriptor.missingInputs.length
          ? "missing_input"
          : previous
            ? "input_changed"
            : "cold_start",
      });
    }
  }

  const reusedDimensions = [];
  const synthesize = [];
  for (const dimension of DIMENSIONS) {
    const metadata = understanding?.incremental?.dimensions?.[dimension];
    if (!understanding?.[dimension] || !metadata) {
      synthesize.push({ dimension, reason: "cold_start" });
      continue;
    }
    const descriptor = dimensionDescriptor(dimension, metadata, hashes, reusedVerdicts);
    const dependencyMissing = descriptor.missingFiles.length || descriptor.missingVerdicts.length;
    const dependencySetEmpty = !descriptor.inputFiles.length && !descriptor.inputVerdictIds.length;
    const repositoryShapeChanged = metadata.fileInventoryFingerprint !== descriptor.fileInventoryFingerprint;
    if (!dependencyMissing && !dependencySetEmpty && !repositoryShapeChanged && metadata.inputFingerprint === descriptor.inputFingerprint) {
      reusedDimensions.push(dimension);
    } else {
      synthesize.push({
        dimension,
        reason: dependencyMissing
          ? "affected_dependency"
          : repositoryShapeChanged
            ? "repository_shape_changed"
          : dependencySetEmpty
            ? "missing_dependencies"
            : "input_changed",
        missingFiles: descriptor.missingFiles,
        missingVerdicts: descriptor.missingVerdicts,
      });
    }
  }

  return {
    schemaVersion: PHASE2_INCREMENTAL_VERSION,
    sourceGenerationId,
    verdicts: { reuse, verify },
    dimensions: { reuse: reusedDimensions, synthesize },
    complete: verify.length === 0 && synthesize.length === 0,
  };
}

export function requeueChangedVerdicts({ graph, queue, verdictEnvelope }) {
  return buildIncrementalPhase2Plan({ graph, queue, verdictEnvelope, understanding: null }).verdicts.verify;
}

export function sealPhase2Artifacts({ graph, queue, verdictEnvelope, understanding }) {
  const sourceGenerationId = graph?.manifest?.generationId;
  if (!sourceGenerationId) throw new Error("phase2_seal_invalid: graph generationId is missing");
  const hashes = graphFileHashes(graph);
  const suppliedVerdicts = new Map(
    (verdictEnvelope?.verdicts ?? []).map((item) => [item.claimId ?? item.id, item]),
  );
  const sealedVerdicts = [];
  for (const claim of eligibleClaims(queue)) {
    const supplied = suppliedVerdicts.get(claim.id);
    if (!supplied) throw new Error(`phase2_seal_invalid: verdict missing for ${claim.id}`);
    const descriptor = claimDescriptor(claim, hashes);
    if (descriptor.missingInputs.length) {
      throw new Error(`phase2_seal_invalid: evidence missing for ${claim.id}: ${descriptor.missingInputs.join(", ")}`);
    }
    const sealedVerdict = {
      ...supplied,
      claimId: claim.id,
      inputFiles: descriptor.inputFiles,
      inputFingerprint: descriptor.inputFingerprint,
    };
    sealedVerdict.reconciliationFingerprint = reconcileInputFingerprint(sealedVerdict);
    sealedVerdicts.push(sealedVerdict);
  }
  const verdictsById = new Map(sealedVerdicts.map((item) => [item.claimId, item]));

  const sealedDimensionMetadata = {};
  for (const dimension of DIMENSIONS) {
    if (!understanding?.[dimension]) {
      throw new Error(`phase2_seal_invalid: understanding.${dimension} is missing`);
    }
    const metadata = understanding?.incremental?.dimensions?.[dimension];
    if (!metadata) throw new Error(`phase2_seal_invalid: incremental metadata missing for ${dimension}`);
    const descriptor = dimensionDescriptor(dimension, metadata, hashes, verdictsById);
    if (!descriptor.inputFiles.length && !descriptor.inputVerdictIds.length) {
      throw new Error(`phase2_seal_invalid: ${dimension} declares no dependencies`);
    }
    if (descriptor.missingFiles.length || descriptor.missingVerdicts.length) {
      throw new Error(
        `phase2_seal_invalid: ${dimension} has missing dependencies: ${[
          ...descriptor.missingFiles,
          ...descriptor.missingVerdicts,
        ].join(", ")}`,
      );
    }
    sealedDimensionMetadata[dimension] = {
      inputFiles: descriptor.inputFiles,
      inputVerdictIds: descriptor.inputVerdictIds,
      inputFingerprint: descriptor.inputFingerprint,
      fileInventoryFingerprint: descriptor.fileInventoryFingerprint,
    };
  }

  return {
    verdicts: {
      ...verdictEnvelope,
      sourceGenerationId,
      incrementalSchemaVersion: PHASE2_INCREMENTAL_VERSION,
      verdicts: sealedVerdicts,
    },
    understanding: {
      ...understanding,
      sourceGenerationId,
      incremental: {
        schemaVersion: PHASE2_INCREMENTAL_VERSION,
        dimensions: sealedDimensionMetadata,
      },
    },
  };
}
