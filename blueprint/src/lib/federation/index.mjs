// Scoped federation: repositories retain independent generation/identity spaces.
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
