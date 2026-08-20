// D35: scoped federation — each repository graph stays independently scoped
// by repo ID and generation. Federation composes slices; it never raw-merges
// stores. Cross-repo answers retain repository boundaries.

import { BlueprintError } from "../application/errors.mjs";

export function composeFederatedSlices(slices = []) {
  const repos = [];
  for (const slice of slices) {
    if (!slice.repoId || !slice.generationId) {
      throw new BlueprintError("slice_incomplete", "each federated slice needs repoId and generationId");
    }
    if (repos.some((existing) => existing.repoId === slice.repoId && existing.generationId !== slice.generationId)) {
      throw new BlueprintError("generation_ambiguity", `repo ${slice.repoId} contributed two generations`);
    }
    repos.push({ repoId: slice.repoId, generationId: slice.generationId, resultCount: slice.results?.length ?? 0 });
  }
  return {
    schemaVersion: 1,
    kind: "federated",
    repos,
    results: slices.flatMap((slice) => slice.results ?? []),
  };
}

export function isRepoAllowed(slice, allowedRepoIds) {
  return allowedRepoIds.includes(slice.repoId);
}
