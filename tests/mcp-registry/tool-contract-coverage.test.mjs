import assert from "node:assert/strict";
import test from "node:test";
import {
  computeToolContractCoverage,
  CONTRACT_COVERAGE_GAP,
  CONTRACT_COVERAGE_OPERATIONS_REGISTRY,
} from "../../scripts/release/registry/tool-contract-coverage.mjs";
import { TOOLS } from "../../mcp/server.mjs";
import { OPERATIONS } from "../../engine/crates/membrane-protocol/bindings/operations.mjs";

test("every real MCP tool from mcp/server.mjs appears exactly once, sorted by name", () => {
  const coverage = computeToolContractCoverage();
  const names = coverage.map((entry) => entry.name);
  assert.deepEqual(names, names.slice().sort(), "coverage list must be sorted");
  assert.deepEqual(new Set(names), new Set(TOOLS.map((tool) => tool.name)), "coverage names must equal the live TOOLS list exactly (no invented, no missing)");
  assert.equal(names.length, TOOLS.length, "no duplicate tool entries");
});

test("a tool with a matching OPERATIONS entry is marked operations_registry, with no gapReason", () => {
  const coverage = computeToolContractCoverage();
  const operationNames = new Set(OPERATIONS.map((operation) => operation.name));
  for (const entry of coverage) {
    if (operationNames.has(entry.name)) {
      assert.equal(entry.contractCoverage, CONTRACT_COVERAGE_OPERATIONS_REGISTRY, `${entry.name} has an OPERATIONS entry and must be marked ${CONTRACT_COVERAGE_OPERATIONS_REGISTRY}`);
      assert.equal(entry.gapReason, undefined, `${entry.name} is contract-covered and must not carry a gapReason`);
    }
  }
});

test("membrane_cortex is the known, real, currently uncontracted gap", () => {
  // Ground truth, not an assumption: membrane_cortex is a real tool
  // (mcp/server.mjs TOOLS) with no matching name in the OPERATIONS
  // cross-operation registry (engine/crates/membrane-protocol/bindings/
  // operations.mjs) as of this task. If a future task adds a
  // "membrane_cortex" OPERATIONS entry with real golden fixtures, this
  // assertion -- and the corresponding server.json entry -- must be
  // updated together; until then, server.json must not silently claim
  // full contract coverage for it.
  assert.ok(TOOLS.some((tool) => tool.name === "membrane_cortex"), "precondition: membrane_cortex must be a real exposed tool for this test to be meaningful");
  assert.ok(!OPERATIONS.some((operation) => operation.name === "membrane_cortex"), "precondition: membrane_cortex must currently be absent from OPERATIONS for this test to be meaningful");

  const coverage = computeToolContractCoverage();
  const entry = coverage.find((candidate) => candidate.name === "membrane_cortex");
  assert.ok(entry, "membrane_cortex must appear in the coverage list");
  assert.equal(entry.contractCoverage, CONTRACT_COVERAGE_GAP);
  assert.equal(typeof entry.gapReason, "string");
  assert.ok(entry.gapReason.length > 0);
  assert.match(entry.gapReason, /OPERATIONS/);
});

test("a synthetic tool absent from a synthetic operations registry is a declared gap, not fabricated coverage", () => {
  const coverage = computeToolContractCoverage({
    tools: [{ name: "synthetic_tool_a" }, { name: "synthetic_tool_b" }],
    operations: [{ name: "synthetic_tool_a" }],
  });
  assert.deepEqual(coverage, [
    { name: "synthetic_tool_a", contractCoverage: CONTRACT_COVERAGE_OPERATIONS_REGISTRY },
    {
      name: "synthetic_tool_b",
      contractCoverage: CONTRACT_COVERAGE_GAP,
      gapReason:
        "synthetic_tool_b is exposed as a real MCP tool in mcp/server.mjs but has no entry in the OPERATIONS cross-operation registry (engine/crates/membrane-protocol/bindings/operations.mjs): no schemaVersion/errorVersion pair, no golden success/error fixtures, and no closed error-code taxonomy validated by validateOperationFixtures().",
    },
  ]);
});
