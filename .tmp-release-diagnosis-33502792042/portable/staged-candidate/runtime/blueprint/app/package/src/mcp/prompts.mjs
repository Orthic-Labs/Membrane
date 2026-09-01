// D31: MCP prompts. Prompts reference tools; they never embed repository
// prose as instructions.

export const PROMPTS = Object.freeze([
  {
    name: "recall-before-change",
    description: "Establish current repository context before changing code.",
    toolRefs: ["blueprint_recall", "blueprint_search", "blueprint_impact"],
  },
  {
    name: "debug",
    description: "Trace a failure to its evidence.",
    toolRefs: ["blueprint_search", "blueprint_expand", "blueprint_doc_truth"],
  },
  {
    name: "review",
    description: "Review a change against repository truth.",
    toolRefs: ["blueprint_impact", "blueprint_doc_truth", "blueprint_status"],
  },
  {
    name: "architecture-validation",
    description: "Validate an architecture decision against the graph.",
    toolRefs: ["blueprint_recall", "blueprint_search", "blueprint_impact"],
  },
  {
    name: "documentation-reconciliation",
    description: "Reconcile docs with current code truth.",
    toolRefs: ["blueprint_doc_truth", "blueprint_search"],
  },
  {
    name: "onboarding",
    description: "Onboard to a repository's structure and truth.",
    toolRefs: ["blueprint_recall", "blueprint_status", "blueprint_search"],
  },
]);

export function promptByName(name) {
  return PROMPTS.find((prompt) => prompt.name === name) ?? null;
}
