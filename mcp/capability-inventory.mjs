import { readFile } from "node:fs/promises";
import { TOOLS, TOOL_OUTPUT_SCHEMA } from "./server.mjs";

export async function buildCapabilityInventory({ matrixPath, freezePath } = {}) {
  const matrix = JSON.parse(await readFile(matrixPath ?? new URL("../../sentinel/hooks/membrane-capability-matrix.json", import.meta.url), "utf8"));
  const freeze = JSON.parse(await readFile(freezePath ?? new URL("../../docs/membrane/federation-freeze-v1.json", import.meta.url), "utf8"));
  const toolTests = (name) => {
    if (["membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad"].includes(name)) return ["mcp/working-context.test.mjs", "mcp/server-durable.test.mjs"];
    if (["membrane_knowledge_propose", "membrane_feedback"].includes(name)) return ["mcp/server.test.mjs", "mcp/server-durable.test.mjs"];
    return ["mcp/server.test.mjs"];
  };
  const capabilities = [
    ...TOOLS.map(({ name }) => ({ capability: `mcp.${name}`, status: "shipped", test_ids: toolTests(name), artifact: "mcp/server.mjs", platforms: ["macOS", "Windows"] })),
    ...Object.entries(matrix.hosts).map(([id]) => ({ capability: `adapter.${id}`, status: id === "generic_mcp" ? "shipped" : "partial", test_ids: id === "generic_mcp" ? ["mcp/adapters.test.mjs"] : [], artifact: "sentinel/hooks/membrane-capability-matrix.json", platforms: id === "generic_mcp" ? ["macOS", "Windows"] : [] })),
  ];
  for (const capability of capabilities) {
    if (capability.status === "shipped" && (!capability.test_ids.length || !capability.artifact || !capability.platforms.length)) throw new Error(`shipped capability lacks proof: ${capability.capability}`);
  }
  return {
    schema: "orthic.capability-inventory.v1",
    vocabulary: { current: ["Membrane", "Crypt", "Sentinel", "Morph", "Cortex"], compatibility: ["RightContext", "Crypt", "Sentinel", "Morph", "Blueprint"] },
    labels: ["shipped", "partial", "unwired", "design", "deprecated"],
    exercised_path_rule: "No shipped claim without an exercised-path test ID.",
    mcp: { tools: TOOLS.map(({ name, inputSchema }) => ({ name, inputSchema })), output_schema: TOOL_OUTPUT_SCHEMA },
    adapters: Object.fromEntries(Object.entries(matrix.hosts).map(([id, host]) => [id, {
      level: host.max_honest_level,
      mechanisms: { injection: host.injection || [], tool_receipts: host.tool_receipts || [], response_gate: host.response_gate || [] },
      ...(host.inherits ? { inherits: host.inherits } : {}),
    }])),
    support_tiers: matrix.support_tiers,
    contract_freeze: freeze.canonical,
    capabilities,
    source_files: ["mcp/server.mjs", "sentinel/hooks/membrane-capability-matrix.json", "docs/membrane/federation-freeze-v1.json"],
  };
}
