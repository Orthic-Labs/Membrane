// D31: MCP prompts. Prompts reference tools; they never embed repository
// prose as instructions.

export const PROMPTS = Object.freeze([
  {
    name: "orient-before-change",
    description: "Establish current repository context before changing code.",
    toolRefs: ["cortex_orient", "cortex_search", "cortex_impact"],
  },
  {
    name: "debug",
    description: "Trace a failure to its evidence.",
    toolRefs: ["cortex_search", "cortex_expand", "cortex_doc_truth"],
  },
  {
    name: "review",
    description: "Review a change against repository truth.",
    toolRefs: ["cortex_impact", "cortex_doc_truth", "cortex_status"],
  },
  {
    name: "architecture-validation",
    description: "Validate an architecture decision against the graph.",
    toolRefs: ["cortex_orient", "cortex_search", "cortex_impact"],
  },
  {
    name: "documentation-reconciliation",
    description: "Reconcile docs with current code truth.",
    toolRefs: ["cortex_doc_truth", "cortex_search"],
  },
  {
    name: "onboarding",
    description: "Onboard to a repository's structure and truth.",
    toolRefs: ["cortex_orient", "cortex_status", "cortex_search"],
  },
]);

export function promptByName(name) {
  return PROMPTS.find((prompt) => prompt.name === name) ?? null;
}
