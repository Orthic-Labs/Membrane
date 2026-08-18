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
    nextOperation: "cortex init",
  }),
  graph_missing: Object.freeze({
    retryable: true,
    summary: "No graph store exists for this repository; build the graph to enable queries.",
    nextOperation: "cortex build",
  }),
  stale_blocked: Object.freeze({
    retryable: true,
    summary: "Re-orient against the current generation, or pass allowStale to accept known-stale evidence.",
    nextOperation: "cortex_orient",
  }),
  generation_mismatch: Object.freeze({
    retryable: true,
    summary: "The graph advanced after orientation; re-orient to obtain a current receipt.",
    nextOperation: "cortex_orient",
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
    summary: "No complete graph generation is available for orientation; build the graph first.",
    nextOperation: "cortex build",
  }),
  missing_generation: Object.freeze({
    retryable: true,
    summary: "No sealed generation exists; build the graph before querying.",
    nextOperation: "cortex build",
  }),
  receipt_reuse: Object.freeze({
    retryable: false,
  }),
  oriented: Object.freeze({
    retryable: false,
  }),
  oriented_stale: Object.freeze({
    retryable: true,
    summary: "Orientation established under stale graph state; rebuild to refresh the generation.",
    nextOperation: "cortex build",
  }),
  oriented_indeterminate: Object.freeze({
    retryable: true,
    summary: "Orientation established under indeterminate graph state; rebuild to get a determinate generation.",
    nextOperation: "cortex build",
  }),
  missing_receipt_id: Object.freeze({
    retryable: false,
    summary: "expand/revoke requires a receiptId; call orient first and pass the returned receiptId.",
    nextOperation: "cortex orient",
  }),
  receipt_not_found: Object.freeze({
    retryable: false,
    summary: "No receipt matches the receiptId; call orient to establish one.",
    nextOperation: "cortex orient",
  }),
  receipt_revoked: Object.freeze({
    retryable: false,
    summary: "Receipt is revoked; re-orient with force=true or a new session/task.",
    nextOperation: "cortex orient",
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
    summary: "Graph generation changed after orientation; re-orient against the current generation before expanding.",
    nextOperation: "cortex orient",
  }),
  expanded: Object.freeze({
    retryable: false,
  }),
  no_receipt: Object.freeze({
    retryable: false,
    summary: "No orientation receipt found for the active session/task/repo; call orient.",
    nextOperation: "cortex orient",
  }),
  receipt_active: Object.freeze({
    retryable: false,
  }),
  already_revoked: Object.freeze({
    retryable: false,
  }),
  revoked: Object.freeze({
    retryable: false,
    summary: "Orientation receipt revoked; call orient before further orientation-dependent work.",
    nextOperation: "cortex orient",
  }),
});

export class CortexError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = "CortexError";
    this.code = code;
    this.details = details;
    const meta = ERROR_METADATA[code];
    this.retryable = Boolean(meta?.retryable ?? false);
    this.remediation = meta?.nextOperation
      ? { summary: meta.summary, nextOperation: meta.nextOperation, arguments: {} }
      : null;
  }
}

export function fail(code, message, details) {
  throw new CortexError(code, message, details);
}
