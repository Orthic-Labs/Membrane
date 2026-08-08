// MBR-907: computes, from the two REAL source-of-truth modules (never
// hand-typed, never duplicated), which MCP tools this server actually
// exposes and which of those have a matching entry in the OPERATIONS
// cross-operation registry.
//
// Sources of truth (both read-only; neither is modified by this task):
//   - `TOOLS` in `mcp/server.mjs` -- the exact tool list the running MCP
//     server advertises via `tools/list` (MBR-301/live server contract).
//   - `OPERATIONS` in
//     `engine/crates/membrane-protocol/bindings/operations.mjs` -- the
//     cross-operation registry that pins, per operation, a schemaVersion,
//     an errorVersion, golden success/error fixtures, and a closed
//     error-code taxonomy (validated by `validateOperationFixtures`).
//
// A tool present in TOOLS but absent from OPERATIONS is a real,
// currently uncontracted MCP surface: it has no golden fixtures and no
// closed error-code taxonomy tracked by that mechanism. This module marks
// it "gap" rather than silently reporting it as covered. Today that is
// exactly `membrane_cortex`: mcp/server.mjs (TOOL_DEFINITIONS, line ~59;
// dispatch at `if (name === "membrane_cortex")`) exposes it as a real
// tool, but OPERATIONS (engine/crates/membrane-protocol/bindings/
// operations.mjs) has no entry named "membrane_cortex" -- confirmed by
// this module's own test (tests/mcp-registry/tool-contract-coverage.test.mjs)
// importing both real modules directly, not a fixture.
//
// This module never invents a tool name or a coverage verdict: it is a
// pure function of the two imported registries, so registry metadata
// generated from it (see build-server-tools.mjs) cannot drift from what
// the server and the operations registry actually contain without this
// module's own test failing first.
import { TOOLS } from "../../../mcp/server.mjs";
import { OPERATIONS } from "../../../engine/crates/membrane-protocol/bindings/operations.mjs";

export const CONTRACT_COVERAGE_OPERATIONS_REGISTRY = "operations_registry";
export const CONTRACT_COVERAGE_GAP = "gap";

function gapReason(name) {
  return (
    `${name} is exposed as a real MCP tool in mcp/server.mjs but has no entry in the OPERATIONS ` +
    `cross-operation registry (engine/crates/membrane-protocol/bindings/operations.mjs): no ` +
    `schemaVersion/errorVersion pair, no golden success/error fixtures, and no closed error-code ` +
    `taxonomy validated by validateOperationFixtures().`
  );
}

/**
 * Returns one entry per real MCP tool, sorted by name, each either:
 *   { name, contractCoverage: "operations_registry" }
 * or:
 *   { name, contractCoverage: "gap", gapReason: "<why>" }
 *
 * Accepts `tools`/`operations` overrides only for testing against
 * synthetic registries; the exported defaults are the real, live modules.
 */
export function computeToolContractCoverage({ tools = TOOLS, operations = OPERATIONS } = {}) {
  const operationNames = new Set(operations.map((operation) => operation.name));
  return tools
    .map((tool) => tool.name)
    .slice()
    .sort()
    .map((name) =>
      operationNames.has(name)
        ? { name, contractCoverage: CONTRACT_COVERAGE_OPERATIONS_REGISTRY }
        : { name, contractCoverage: CONTRACT_COVERAGE_GAP, gapReason: gapReason(name) },
    );
}

if (process.argv[1] === new URL(import.meta.url).pathname) {
  process.stdout.write(`${JSON.stringify(computeToolContractCoverage(), null, 2)}\n`);
}
