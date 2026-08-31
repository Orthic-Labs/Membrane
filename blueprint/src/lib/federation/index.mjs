// D35: scoped federation — each repository graph stays independently scoped
// by repo ID and generation. Federation composes slices; it never raw-merges
// stores. Cross-repo answers retain repository boundaries.

import { BlueprintError } from "../application/errors.mjs";

export function composeFederatedSlices(slices = []) {
  const repos = [];
  const seen = new Set();
  for (const slice of slices) {
    if (!slice.repoId || !slice.generationId) {
      throw new BlueprintError("slice_incomplete", "each federated slice needs repoId and generationId");
    }
    if (repos.some((existing) => existing.repoId === slice.repoId && existing.generationId !== slice.generationId)) {
      throw new BlueprintError("generation_ambiguity", `repo ${slice.repoId} contributed two generations`);
    }
    if (seen.has(slice.repoId)) throw new BlueprintError("repository_duplicate", `repo ${slice.repoId} contributed more than one slice`);
    seen.add(slice.repoId);
    repos.push({ repoId: slice.repoId, repoRoot: slice.repoRoot ?? null, generationId: slice.generationId, receiptId: slice.receiptId ?? null, resultCount: slice.results?.length ?? 0 });
  }
  return {
    schemaVersion: 1,
    kind: "federated",
    repos,
    plannerAuthority: "external",
    selection: "unranked_repository_slices",
    slices: slices.map((slice) => ({
      repoId: slice.repoId,
      repoRoot: slice.repoRoot ?? null,
      generationId: slice.generationId,
      receiptId: slice.receiptId ?? null,
      results: slice.results ?? [],
      omissions: slice.omissions ?? [],
    })),
    // Compatibility projection remains repository-wrapped. Raw node/result
    // objects are never flattened into one synthetic node space.
    results: slices.map((slice) => ({ repoId: slice.repoId, generationId: slice.generationId, results: slice.results ?? [] })),
  };
}

export function isRepoAllowed(slice, allowedRepoIds) {
  return allowedRepoIds.includes(slice.repoId);
}

export async function routeFederatedQuery({ repositories = [], allowedRepoIds = [], operation, input = {}, querySlice }) {
  if (!Array.isArray(repositories) || repositories.length < 1 || repositories.length > 16) {
    throw new BlueprintError("federation_bounds_invalid", "federation requires 1 to 16 explicit repositories");
  }
  if (typeof querySlice !== "function") throw new BlueprintError("federation_router_missing", "federation query router is unavailable");
  if (!new Set(["search", "recall", "impact", "architecture"]).has(operation)) {
    throw new BlueprintError("federation_operation_invalid", `unsupported federated operation ${operation}`);
  }
  const ids = new Set();
  for (const repository of repositories) {
    if (!repository?.repoId || ids.has(repository.repoId)) throw new BlueprintError("repository_duplicate", "each federated repository needs one unique repoId");
    ids.add(repository.repoId);
    if (!isRepoAllowed(repository, allowedRepoIds)) throw new BlueprintError("repository_not_allowed", `repo ${repository.repoId} is outside the explicit federation allowlist`);
  }
  const slices = await Promise.all(repositories.map(async (repository) => {
    try {
      const result = await querySlice(repository, operation, input);
      return {
        repoId: repository.repoId,
        repoRoot: result.repoRoot ?? repository.repoRoot ?? null,
        generationId: result.generationId,
        receiptId: result.freshnessReceipt?.receiptId ?? null,
        results: [result],
        omissions: [],
      };
    } catch (error) {
      return {
        repoId: repository.repoId,
        repoRoot: repository.repoRoot ?? null,
        generationId: repository.generation ?? "unavailable",
        receiptId: null,
        results: [],
        omissions: [{ reason: "repository_query_failed", code: error?.code ?? "internal_error", message: error?.message ?? String(error) }],
      };
    }
  }));
  return composeFederatedSlices(slices);
}
