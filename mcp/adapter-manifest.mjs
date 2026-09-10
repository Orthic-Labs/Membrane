import { readFile } from "node:fs/promises";

// Keep this manifest usable after legacy JavaScript MCP retirement.  Native
// discovery is owned by engine/crates/membrane-mcp/src/tools.rs; this mirrored
// inventory is intentionally limited to names because native Rust remains
// authoritative for schemas & execution.
const NATIVE_TOOL_NAMES = [
  "membrane_context", "membrane_source_read", "membrane_blueprint",
  "membrane_knowledge_propose", "membrane_memory", "membrane_memory_read",
  "membrane_checkpoint_save", "membrane_checkpoint_load",
  "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad",
  "membrane_feedback", "membrane_ledger", "membrane_push_prepare",
  "membrane_push_resolve", "membrane_diagnostic_workspace",
  "membrane_diagnostic_mutation", "membrane_diagnostic_snapshot",
  "membrane_diagnostic_fence", "membrane_diagnostic_capabilities",
  "membrane_diagnostic_baseline", "membrane_diagnostic_provider",
  "membrane_adapt_inspect", "membrane_knowledge_review",
];

const NATIVE_INPUT_SCHEMA = {
  type: "object",
  description: "Schema owned by native membrane-mcp; invoke native discovery for complete constraints.",
};

const NATIVE_OUTPUT_SCHEMA = {
  type: "object",
  description: "Canonical Membrane operation envelope; native membrane-mcp is authoritative.",
};

export async function buildAdapterManifest() {
  // MBR-015: read capability matrix and contract freeze from Membrane's OWN tree
  // (docs/membrane/), never a sibling source path. The installed runtime needs
  // no sibling checkout — both data files are vendored in-repo.
  const matrix = JSON.parse(await readFile(new URL("../docs/membrane/capability-matrix.v1.json", import.meta.url), "utf8"));
  const freeze = JSON.parse(await readFile(new URL("../docs/membrane/federation-freeze-v1.json", import.meta.url), "utf8"));
  return {
    schema: "membrane.adapter-manifest.v1",
    generated_from: ["engine/crates/membrane-mcp/src/tools.rs", "schemas/registry/push-tools.v1.json", "docs/membrane/capability-matrix.v1.json", "docs/membrane/federation-freeze-v1.json"],
    mcp: { tools: NATIVE_TOOL_NAMES.map((name) => ({ name, input_schema: { ...NATIVE_INPUT_SCHEMA } })), output_schema: NATIVE_OUTPUT_SCHEMA },
    adapters: matrix.hosts,
    support_tiers: matrix.support_tiers,
    states: matrix.states,
    contract_freeze: freeze.canonical,
  };
}
