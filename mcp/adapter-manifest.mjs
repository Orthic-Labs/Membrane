import { readFile } from "node:fs/promises";
import { TOOLS, TOOL_OUTPUT_SCHEMA } from "./server.mjs";

export async function buildAdapterManifest() {
  const matrix = JSON.parse(await readFile(new URL("../../forge/hooks/membrane-capability-matrix.json", import.meta.url), "utf8"));
  const freeze = JSON.parse(await readFile(new URL("../../docs/membrane/federation-freeze-v1.json", import.meta.url), "utf8"));
  return {
    schema: "orthic.adapter-manifest.v1",
    generated_from: ["mcp/server.mjs", "forge/hooks/membrane-capability-matrix.json", "docs/membrane/federation-freeze-v1.json"],
    mcp: { tools: TOOLS.map((tool) => ({ name: tool.name, input_schema: tool.inputSchema })), output_schema: TOOL_OUTPUT_SCHEMA },
    adapters: matrix.hosts,
    support_tiers: matrix.support_tiers,
    states: matrix.states,
    contract_freeze: freeze.canonical,
  };
}
