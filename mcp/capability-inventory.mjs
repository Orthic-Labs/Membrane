import { readFile } from "node:fs/promises";
import { TOOLS, TOOL_OUTPUT_SCHEMA } from "./server.mjs";

export async function buildCapabilityInventory({ matrixPath, freezePath } = {}) {
  const matrix = JSON.parse(await readFile(matrixPath ?? new URL("../../tether/hooks/membrane-capability-matrix.json", import.meta.url), "utf8"));
  const freeze = JSON.parse(await readFile(freezePath ?? new URL("../../docs/rightcontext/federation-freeze-v1.json", import.meta.url), "utf8"));
  return {
    schema: "orthic.capability-inventory.v1",
    vocabulary: { current: ["Membrane", "Crypt", "Sentinel", "Morph", "Cortex"], compatibility: ["RightContext", "MemRight", "Tether", "Adapt", "Blueprint"] },
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
    source_files: ["mcp/server.mjs", "tether/hooks/membrane-capability-matrix.json", "docs/rightcontext/federation-freeze-v1.json"],
  };
}
