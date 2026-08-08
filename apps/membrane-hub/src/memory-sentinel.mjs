const esc = value => String(value ?? 'unknown').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const list = (value) => { const ids = Array.isArray(value) ? value : (Array.isArray(value?.ids) ? value.ids : []); const count = Array.isArray(value) ? value.length : (Number.isFinite(value?.count) ? value.count : null); return { count, ids: ids.slice(0, 64) }; };
export function memorySentinelViewModel(snapshot) {
  const data = snapshot?.result?.data || snapshot?.data || snapshot || {};
  const l = data.lifecycle || {};
  const unknown = { state: 'unknown', reason: 'unknown — no evidence', valid: false };
  const count = value => Number.isFinite(value) ? value : null;
  return { schemaVersion: data.schemaVersion || 'memory-sentinel.v1', lifecycle: { active: count(l.active), demoted: count(l.demoted), superseded: count(l.superseded), expired: count(l.expired) }, proposals: list(data.proposals), contradictions: list(data.contradictions), scratchpad: list(data.scratchpad), working: list(data.working), taskCriteria: list(data.taskCriteria), evidence: { ...unknown, ...(data.evidence || {}) }, gate: { state: 'unknown', reason: 'unknown — no authoritative gate evidence', authoritative: false, ...(data.gate || {}) } };
}
export function renderMemorySentinel(snapshot, root) {
  const vm = memorySentinelViewModel(snapshot);
  const row = (name, value) => `<dt>${esc(name)}</dt><dd>${esc(value.count ?? 'unknown')} · ${value.ids.length ? value.ids.map(esc).join(', ') : 'no evidence'}</dd>`;
  root.innerHTML = `<section class="memory-sentinel" aria-label="Memory Sentinel"><header><h2>Memory Sentinel</h2><small>read-only projection · schema: ${esc(vm.schemaVersion)}</small></header><dl>${row('Active', { count: vm.lifecycle.active, ids: [] })}${row('Demoted', { count: vm.lifecycle.demoted, ids: [] })}${row('Superseded', { count: vm.lifecycle.superseded, ids: [] })}${row('Expired', { count: vm.lifecycle.expired, ids: [] })}${row('Proposals', vm.proposals)}${row('Contradictions', vm.contradictions)}${row('Scratchpad expiry', vm.scratchpad)}${row('Working expiry', vm.working)}${row('Task criteria', vm.taskCriteria)}</dl><p>Evidence: <strong>${esc(vm.evidence.state)}</strong> · ${esc(vm.evidence.reason)} · valid: ${esc(vm.evidence.valid)}</p><p>Gate: <strong>${esc(vm.gate.state)}</strong> · ${esc(vm.gate.reason)}${vm.gate.authoritative ? ' · authoritative' : ' · authority unavailable'}</p></section>`;
}
