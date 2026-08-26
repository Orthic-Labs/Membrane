// Error metadata grounded in the codes thrown by lib/application/** and the
// reasonCode values emitted by lib/admission.mjs. Lookup is additive: legacy
// fields (code/message/details) are preserved untouched and retryable plus
// remediation are attached when the code is known.
//
// retryable  — whether re-running the operation can succeed without user edits.
// summary    — actionable one-line explanation of the failure.
// nextOperation — the concrete next command/action, or absent for no-op codes.
const ERROR_METADATA = Object.freeze({
  // --- lib/application/** thrown codes ---
  request_cancelled: Object.freeze({
    retryable: false,
  }),
  root_not_enrolled: Object.freeze({
    retryable: false,
    summary: "The requested root is not enrolled; enroll it before querying.",
    nextOperation: "blueprint init",
  }),
  graph_missing: Object.freeze({
    retryable: true,
    summary: "No graph store exists for this repository; build the graph to enable queries.",
    nextOperation: "blueprint build",
  }),
  schema_mismatch: Object.freeze({
    retryable: true,
    summary: "The sealed Blueprint generation does not match the current schema; rebuild it.",
    nextOperation: "blueprint build",
  }),
  service_unavailable: Object.freeze({
    retryable: false,
    summary: "The requested Blueprint service operation is unavailable.",
  }),
  stale_blocked: Object.freeze({
    retryable: true,
    summary: "Recall against the current generation, or pass allowStale to accept known-stale evidence.",
    nextOperation: "blueprint_recall",
  }),
  generation_mismatch: Object.freeze({
    retryable: true,
    summary: "The graph advanced after recall; recall to obtain a current receipt.",
    nextOperation: "blueprint_recall",
  }),
  anchor_not_found: Object.freeze({
    retryable: false,
    summary: "No graph node matches the anchor; use an exact node id, name, path, or file:/symbol: reference.",
    nextOperation: "re-run with a valid anchor",
  }),
  anchor_ambiguous: Object.freeze({
    retryable: false,
    summary: "The anchor matches multiple nodes; narrow it to a qualified name or exact node id.",
    nextOperation: "re-run with a more specific anchor",
  }),
  node_not_found: Object.freeze({
    retryable: false,
    summary: "No graph node matches the nodeId in the current generation; verify the identifier.",
    nextOperation: "re-run with a valid nodeId",
  }),
  root_escape: Object.freeze({
    retryable: false,
  }),
  query_required: Object.freeze({
    retryable: false,
    summary: "A non-empty query is required; pass query or task.",
    nextOperation: "re-run with a non-empty query",
  }),
  anchor_required: Object.freeze({
    retryable: false,
    summary: "An anchor is required; pass a path, symbol, node id, or query.",
    nextOperation: "re-run with an anchor",
  }),

  // --- lib/admission.mjs reasonCode values ---
  missing_graph: Object.freeze({
    retryable: true,
    summary: "No complete graph generation is available for recall; build the graph first.",
    nextOperation: "blueprint build",
  }),
  missing_generation: Object.freeze({
    retryable: true,
    summary: "No sealed generation exists; build the graph before querying.",
    nextOperation: "blueprint build",
  }),
  receipt_reuse: Object.freeze({
    retryable: false,
  }),
  recalled: Object.freeze({
    retryable: false,
  }),
  recalled_stale: Object.freeze({
    retryable: true,
    summary: "Recall established under stale graph state; rebuild to refresh the generation.",
    nextOperation: "blueprint build",
  }),
  recalled_indeterminate: Object.freeze({
    retryable: true,
    summary: "Recall established under indeterminate graph state; rebuild to get a determinate generation.",
    nextOperation: "blueprint build",
  }),
  missing_receipt_id: Object.freeze({
    retryable: false,
    summary: "expand/revoke requires a receiptId; call recall first and pass the returned receiptId.",
    nextOperation: "blueprint recall",
  }),
  receipt_not_found: Object.freeze({
    retryable: false,
    summary: "No receipt matches the receiptId; call recall to establish one.",
    nextOperation: "blueprint recall",
  }),
  receipt_revoked: Object.freeze({
    retryable: false,
    summary: "Receipt is revoked; recall with force=true or a new session/task.",
    nextOperation: "blueprint recall",
  }),
  absolute_path_rejected: Object.freeze({
    retryable: false,
    summary: "expand rejects absolute self-approved paths; pass a relative repo path or graph query.",
    nextOperation: "re-run expand with a relative path",
  }),
  missing_expand_query: Object.freeze({
    retryable: false,
    summary: "expand requires a path, symbol, or query grounded in the graph.",
    nextOperation: "re-run expand with path, symbol, or query",
  }),
  generation_changed: Object.freeze({
    retryable: true,
    summary: "Graph generation changed after recall; recall against the current generation before expanding.",
    nextOperation: "blueprint recall",
  }),
  expanded: Object.freeze({
    retryable: false,
  }),
  no_receipt: Object.freeze({
    retryable: false,
    summary: "No recall receipt found for the active session/task/repo; call recall.",
    nextOperation: "blueprint recall",
  }),
  receipt_active: Object.freeze({
    retryable: false,
  }),
  already_revoked: Object.freeze({
    retryable: false,
  }),
  revoked: Object.freeze({
    retryable: false,
    summary: "Recall receipt revoked; call recall before further recall-dependent work.",
    nextOperation: "blueprint recall",
  }),
});

export class BlueprintError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = "BlueprintError";
    this.code = code;
    this.details = details;
    const meta = ERROR_METADATA[code];
    this.retryable = Boolean(meta?.retryable ?? false);
    this.remediation = details?.remediation ?? (meta?.nextOperation
      ? { summary: meta.summary, nextOperation: meta.nextOperation, arguments: {} }
      : null);
  }
}

export function fail(code, message, details) {
  throw new BlueprintError(code, message, details);
}
