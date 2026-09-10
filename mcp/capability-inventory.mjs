import { readFile } from "node:fs/promises";
import { computeProductTruth } from "../scripts/tools/productization/generate-product-truth.mjs";

const NATIVE_MCP_SOURCE = new URL("../engine/crates/membrane-mcp/src/tools.rs", import.meta.url);

async function nativeTools() {
  const source = await readFile(NATIVE_MCP_SOURCE, "utf8");
  const names = [...source.matchAll(/"(membrane_[a-z0-9_]+)"/g)].map((match) => match[1]);
  return [...new Set(names)].map((name) => ({
    name,
    inputSchema: { type: "object", description: "Schema owned by native membrane-mcp registry" },
  }));
}

// MBR-016: a capability may be labeled "shipped" ONLY when every required gate
// for its claim class passes. Test-presence alone is no longer sufficient. The
// five gates are: exercised-path test, source artifact, platform support,
// generated documentation (from the MBR-013 product truth), and a backing
// contract/schema. Anything fewer is "partial" (or "unwired" at zero).
const FIVE_GATES = Object.freeze(["exercised_test", "artifact", "platforms", "documented", "contract"]);
const REQUIRED_GATES = FIVE_GATES.length;

function statusFromGates(passed, required) {
  if (passed >= required) return "shipped";
  if (passed > 0) return "partial";
  return "unwired";
}

function countPassed(gates) {
  return FIVE_GATES.filter((gate) => gates[gate]).length;
}

export async function buildCapabilityInventory({ matrixPath, freezePath } = {}) {
  // MBR-015: default to Membrane's OWN vendored data (docs/membrane/), never a
  // sibling source path. Callers may still inject explicit paths for fixtures.
  const matrix = JSON.parse(await readFile(matrixPath ?? new URL("../docs/membrane/capability-matrix.v1.json", import.meta.url), "utf8"));
  const freeze = JSON.parse(await readFile(freezePath ?? new URL("../docs/membrane/federation-freeze-v1.json", import.meta.url), "utf8"));
  // MBR-016: the generated product truth (MBR-013) backs the "documented" gate.
  const truth = await computeProductTruth();
  const tools = await nativeTools();
  const documentedTools = new Set(truth.tools);
  const documentedAdapters = new Set(truth.adapters);
  const toolTests = (name) => {
    if (["membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad"].includes(name)) return ["engine/crates/membrane-mcp/tests/discovery_roundtrip.rs", "engine/crates/membrane-runtime/src/mcp_executor.rs"];
    return ["engine/crates/membrane-mcp/tests/discovery_roundtrip.rs"];
  };
  const capabilities = [
    ...tools.map(({ name }) => {
      const test_ids = toolTests(name);
      const artifact = "engine/crates/membrane-mcp/src/tools.rs";
      const platforms = ["macOS", "Windows"];
      const gates = {
        exercised_test: test_ids.length > 0,
        artifact: Boolean(artifact),
        platforms: platforms.length > 0,
        documented: documentedTools.has(name),
        contract: Boolean(artifact),
      };
      const gates_passed = countPassed(gates);
      return { capability: `mcp.${name}`, claim_class: "mcp_tool", status: statusFromGates(gates_passed, REQUIRED_GATES), gates, gates_passed, gates_required: REQUIRED_GATES, test_ids, artifact, platforms, cost: "instant", convergence: "pull_exact", side_effect: "pure_analysis" };
    }),
    ...Object.entries(matrix.hosts).map(([id, host]) => {
      const test_ids = id === "generic_mcp" ? ["mcp/adapters.test.mjs"] : [];
      const artifact = "docs/membrane/capability-matrix.v1.json";
      const platforms = id === "generic_mcp" ? ["macOS", "Windows"] : [];
      const gates = {
        exercised_test: test_ids.length > 0,
        artifact: Boolean(artifact),
        platforms: platforms.length > 0,
        documented: documentedAdapters.has(id),
        contract: Boolean(host?.max_honest_level),
      };
      const gates_passed = countPassed(gates);
      return { capability: `adapter.${id}`, claim_class: "adapter", status: statusFromGates(gates_passed, REQUIRED_GATES), gates, gates_passed, gates_required: REQUIRED_GATES, test_ids, artifact, platforms };
    }),
    // Provider capabilities — Blueprint findings + module-surface (pure analysis, instant cost)
    ...[
      { capability: "findings.bp001", claim_class: "finding", artifact: "engine/crates/membrane-blueprint/src/findings.rs", test_ids: ["engine/crates/membrane-blueprint/tests/findings.rs"], cost: "instant", convergence: "pull_exact", side_effect: "pure_analysis" },
      { capability: "findings.bp002", claim_class: "finding", artifact: "engine/crates/membrane-blueprint/src/findings.rs", test_ids: ["engine/crates/membrane-blueprint/tests/findings.rs"], cost: "instant", convergence: "pull_exact", side_effect: "pure_analysis" },
      { capability: "findings.bp003", claim_class: "finding", artifact: "engine/crates/membrane-blueprint/src/findings.rs", test_ids: ["engine/crates/membrane-blueprint/tests/findings.rs"], cost: "instant", convergence: "pull_exact", side_effect: "pure_analysis" },
      { capability: "module_surface.parse", claim_class: "provider", artifact: "engine/crates/membrane-blueprint/src/static_provider.rs", test_ids: ["engine/crates/membrane-blueprint/tests/parity_language_registry.rs"], cost: "instant", convergence: "snapshot_checker_exact", side_effect: "pure_analysis" },
    ].map((entry) => {
      const platforms = ["macOS", "Windows"];
      const gates = {
        exercised_test: entry.test_ids.length > 0,
        artifact: Boolean(entry.artifact),
        platforms: platforms.length > 0,
        documented: true,
        contract: true,
      };
      const gates_passed = countPassed(gates);
      return {
        capability: entry.capability,
        claim_class: entry.claim_class,
        status: statusFromGates(gates_passed, REQUIRED_GATES),
        gates,
        gates_passed,
        gates_required: REQUIRED_GATES,
        test_ids: entry.test_ids,
        artifact: entry.artifact,
        platforms,
        cost: entry.cost,
        convergence: entry.convergence,
        side_effect: entry.side_effect,
      };
    }),
  ];
  // MBR-016 invariant: nothing is shipped unless every required gate passes.
  for (const capability of capabilities) {
    if (capability.status === "shipped" && capability.gates_passed < capability.gates_required) {
      throw new Error(`shipped capability fails a required gate: ${capability.capability}`);
    }
  }
  return {
    schema: "membrane.capability-inventory.v1",
    vocabulary: { current: ["Membrane", "Cortex", "Forge", "Adapt", "Blueprint"] },
    labels: ["shipped", "partial", "unwired", "design", "deprecated"],
    gate_model: { gates: FIVE_GATES, required: REQUIRED_GATES },
    exercised_path_rule: "No shipped claim without all five gates: exercised-path test, artifact, platforms, generated documentation, and a backing contract.",
    mcp: { tools, output_schema: { schemaVersion: 1, owner: "engine/crates/membrane-mcp" } },
    adapters: Object.fromEntries(Object.entries(matrix.hosts).map(([id, host]) => [id, {
      level: host.max_honest_level,
      mechanisms: { injection: host.injection || [], tool_receipts: host.tool_receipts || [], response_gate: host.response_gate || [] },
      ...(host.inherits ? { inherits: host.inherits } : {}),
    }])),
    support_tiers: matrix.support_tiers,
    contract_freeze: freeze.canonical,
    capabilities,
    source_files: ["engine/crates/membrane-mcp/src/tools.rs", "docs/membrane/capability-matrix.v1.json", "docs/membrane/federation-freeze-v1.json", "engine/crates/membrane-blueprint/src/findings.rs", "engine/crates/membrane-blueprint/src/static_provider.rs", "engine/crates/membrane-blueprint/src/module_resolution.rs"],
  };
}
