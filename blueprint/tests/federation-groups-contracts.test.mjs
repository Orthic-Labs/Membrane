import assert from "node:assert/strict";
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
