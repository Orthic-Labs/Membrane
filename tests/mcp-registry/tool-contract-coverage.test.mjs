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

test("membrane_cortex is contract-covered, closing the gap this test previously pinned", () => {
  // This test formerly asserted membrane_cortex was an uncontracted gap and
  // said that assertion "and the corresponding server.json entry must be
  // updated together" once a task added a real OPERATIONS entry. That task
  // landed: membrane_cortex now has an OPERATIONS entry, a per-operation
  // schema, and golden success/error fixtures. So the assertion is inverted
  // here rather than deleted -- the coverage computation must reflect the
  // registry as it actually is, in either direction. The general mechanism
  // (an uncontracted tool is reported as a declared gap, never as fabricated
  // coverage) stays covered by the synthetic-registry test below, which is
  // where that guarantee belongs -- it does not depend on any one tool's
  // registration state.
  assert.ok(TOOLS.some((tool) => tool.name === "membrane_cortex"), "membrane_cortex must be a real exposed tool");
  assert.ok(OPERATIONS.some((operation) => operation.name === "membrane_cortex"), "membrane_cortex must now be present in OPERATIONS");

  const coverage = computeToolContractCoverage();
  const entry = coverage.find((candidate) => candidate.name === "membrane_cortex");
  assert.ok(entry, "membrane_cortex must appear in the coverage list");
  assert.notEqual(entry.contractCoverage, CONTRACT_COVERAGE_GAP);
  assert.equal(entry.gapReason, undefined, "a contract-covered tool must not carry a gapReason");
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
