// AX P14: declared effect profile per MCP tool. Profiles are hints for hosts;
// the RootRegistry and application service remain the enforcement layer. All
// six tools are read-only graph/document queries: no writes, no project code
// execution, no network, no installs, no destruction, safe to repeat, and
// safe for a host to run without asking the user first.
export const TOOL_EFFECTS = Object.freeze({
  blueprint_orient: Object.freeze({
    reads: ["repository-graph"],
    writes: [],
    executesProjectCode: false,
    network: "none",
    installsSoftware: false,
    destructive: false,
    idempotent: true,
    approval: "not_required",
  }),
  blueprint_search: Object.freeze({
    reads: ["repository-graph"],
    writes: [],
    executesProjectCode: false,
    network: "none",
    installsSoftware: false,
    destructive: false,
    idempotent: true,
    approval: "not_required",
  }),
  blueprint_expand: Object.freeze({
    reads: ["repository-graph"],
    writes: [],
    executesProjectCode: false,
    network: "none",
    installsSoftware: false,
    destructive: false,
    idempotent: true,
    approval: "not_required",
  }),
  blueprint_impact: Object.freeze({
    reads: ["repository-graph"],
    writes: [],
    executesProjectCode: false,
    network: "none",
    installsSoftware: false,
    destructive: false,
    idempotent: true,
    approval: "not_required",
  }),
  blueprint_doc_truth: Object.freeze({
    reads: ["repository-graph", "repository-documents"],
    writes: [],
    executesProjectCode: false,
    network: "none",
    installsSoftware: false,
    destructive: false,
    idempotent: true,
    approval: "not_required",
  }),
  blueprint_status: Object.freeze({
    reads: ["repository-graph"],
    writes: [],
    executesProjectCode: false,
    network: "none",
    installsSoftware: false,
    destructive: false,
    idempotent: true,
    approval: "not_required",
  }),
});
