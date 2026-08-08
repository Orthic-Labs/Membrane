const esc = value => String(value ?? 'unknown').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));

export function normalizeFleet(snapshot) {
  const fleet = snapshot?.result?.data?.fleet || snapshot?.payload?.fleet || snapshot?.fleet;
  if (!fleet || typeof fleet !== 'object') return { state: 'unknown', reason: 'unknown — no evidence', installations: [], replication: [] };
  const text = (value, fallback) => typeof value === 'string' && value.trim() ? value : fallback;
  const byIdentity = (a, b) => String(a.installationId || a.installation_id || a.observerId || a.observer_id || '').localeCompare(String(b.installationId || b.installation_id || b.observerId || b.observer_id || ''));
  return { ...fleet, state: text(fleet.state, 'unknown'), reason: text(fleet.reason, 'unknown — no evidence'), source: text(fleet.source, 'unknown'), evidence: text(fleet.evidence, 'unknown'), resolver: text(fleet.resolver, 'unknown'), installations: (Array.isArray(fleet.installations) ? fleet.installations : []).slice(0, 128).sort(byIdentity), replication: (Array.isArray(fleet.replication) ? fleet.replication : []).slice(0, 128).sort(byIdentity) };
}

export function renderFleet(snapshot, root) {
  const fleet = normalizeFleet(snapshot);
  const item = row => `<tr><td>${esc(row.observerId || row.observer_id || row.installationId || row.installation_id)}</td><td>${esc(row.originId || row.origin_id || '')}</td><td>${esc(row.state)}</td><td>${esc(row.reason)}</td><td>${esc(row.evidence)}</td><td>${esc(row.resolver)}</td></tr>`;
  root.innerHTML = `<section class="fleet" aria-label="Fleet replication"><header><h2>Fleet</h2><span>${esc(fleet.state)}</span></header><p>${esc(fleet.reason)}</p><small>source: ${esc(fleet.source)} · evidence: ${esc(fleet.evidence)} · resolver: ${esc(fleet.resolver)}</small><table><thead><tr><th>Observer</th><th>Origin</th><th>State</th><th>Reason</th><th>Evidence</th><th>Resolver</th></tr></thead><tbody>${[...fleet.installations, ...fleet.replication].map(item).join('') || '<tr><td colspan="6">unknown — no evidence</td></tr>'}</tbody></table></section>`;
}
