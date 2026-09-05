// Entry-point registry for derived architecture/liveness views.
// Explicit source-backed entry points are separated from structural candidates:
// zero inbound degree is useful orientation evidence but never proves execution.

function labels(node) { return new Set((node?.labels ?? []).map((label) => String(label).toLowerCase())); }
function evidence(node) { return Array.isArray(node?.evidence) ? node.evidence.filter(Boolean) : []; }

export function buildEntryPointRegistry(generation, { includeStructuralCandidates = true } = {}) {
  const nodes = generation?.nodes ?? [];
  const edges = generation?.edges ?? [];
  const incoming = new Set(edges.map((edge) => edge.target).filter(Boolean));
  const outgoing = new Set(edges.map((edge) => edge.source).filter(Boolean));
  const rows = [];
  for (const node of nodes) {
    const tagged = node?.entryPoint === true || labels(node).has("entrypoint") || labels(node).has("entry_point");
    if (tagged) {
      rows.push({ id: node.id, node, authority: "explicit", reason: "source_backed_entrypoint_marker", evidence: evidence(node) });
      continue;
    }
    if (includeStructuralCandidates && node?.kind === "symbol" && outgoing.has(node.id) && !incoming.has(node.id)) {
      rows.push({ id: node.id, node, authority: "structural_candidate", reason: "outgoing_with_zero_observed_inbound", evidence: evidence(node) });
    }
  }
  return rows.sort((a, b) => String(a.id).localeCompare(String(b.id)));
}
