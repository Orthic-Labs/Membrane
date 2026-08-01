export const LEGACY_PATHS = ["duplicate_legacy_recall_hook", "implicit_token_endpoint_db_discovery", "free_form_scope_creation", "success_shaped_noop_mcp_writes", "graph_stale_markdown_fallback", "agent_managed_beacon_lifecycle", "old_rightcontext_public_names", "root_only_repository_assumptions", "duplicate_adapter_packet_assembly", "hand_maintained_docs_inventories", "n1d_aliases"];

export function deletionPlan({ stableActivation = false, compatibilityReceipt = null, rollbackBoundary = null } = {}) {
  const deletable = stableActivation && Boolean(compatibilityReceipt) && Boolean(rollbackBoundary);
  return { schema: "orthic.legacy-deletion-plan.v1", status: deletable ? "deletable" : "blocked_until_stable_activation", paths: LEGACY_PATHS, compatibility_receipt: compatibilityReceipt, rollback_boundary: rollbackBoundary, durable_ids_rekeyed: false };
}

export function n1dEvidenceGate({ macosCycle = null, windowsCycle = null, compatibilityReceipt = null, rollbackReceipt = null } = {}) {
  const missing = [];
  for (const [platform, cycle] of [["macOS", macosCycle], ["Windows", windowsCycle]]) {
    if (!cycle?.clean_daily_cycle) missing.push(`${platform} clean daily cycle`);
    if (!cycle?.alias_readback) missing.push(`${platform} alias compatibility readback`);
  }
  if (!compatibilityReceipt) missing.push("compatibility receipt");
  if (!rollbackReceipt) missing.push("rollback receipt");
  return {
    schema: "orthic.n1d-evidence-gate.v1",
    status: missing.length === 0 ? "ready_to_delete" : "blocked",
    missing,
    delete_targets: missing.length === 0 ? ["legacy CLI aliases", "legacy service aliases", "legacy environment aliases"] : [],
    durable_ids_rekeyed: false,
    rollback_receipt: rollbackReceipt,
  };
}
