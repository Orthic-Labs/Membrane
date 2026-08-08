const esc = value => String(value ?? 'unknown').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const STATES = ['installed', 'active', 'delivering', 'enforced', 'proven'];
const unknown = () => ({ state: 'unknown', mechanism: 'unavailable', evidence: 'unknown — no evidence' });
const status = value => typeof value === 'string' ? { state: value, mechanism: 'declared', evidence: 'declared status' } : { ...unknown(), ...(value || {}) };

export function agentsAdaptersViewModel(snapshot) {
  const data = snapshot?.result?.data || snapshot?.data || snapshot || {};
  const adapters = Array.isArray(data.adapters) ? data.adapters : (Array.isArray(data.agentsAdapters) ? data.agentsAdapters : []);
  return { schemaVersion: data.schemaVersion || 'agent-adapters.v1', adapters: adapters.slice(0, 64).map(item => {
    const declared = item.declaredCapability || item.declared_capability || 'unknown';
    const client = item.clientCapability || item.client_capability || 'unknown';
    const below = /^L\d+$/.test(client) && /^L\d+$/.test(declared) && Number(client.slice(1)) < Number(declared.slice(1));
    const comparable = /^L\d+$/.test(client) && /^L\d+$/.test(declared);
    const capability = below ? { state: 'degraded', mechanism: 'client capability below declared capability', evidence: `client ${client} < declared ${declared}` } : comparable ? { state: 'available', mechanism: 'capability comparison', evidence: `client ${client} meets declared ${declared}` } : unknown();
    const states = Object.fromEntries(STATES.map(key => [key, status(item[key])]));
    return { id: item.id || 'unknown', client: item.client || 'unknown', device: item.device || 'unknown', declaredCapability: declared, clientCapability: client, capability, ...states };
  }) };
}

export function renderAgentsAdapters(snapshot, root) {
  const vm = agentsAdaptersViewModel(snapshot);
  const rows = vm.adapters.map(a => `<article><h3>${esc(a.client)} · ${esc(a.device)}</h3><small>${esc(a.id)} · client ${esc(a.clientCapability)} / declared ${esc(a.declaredCapability)}</small><p>Capability: <strong>${esc(a.capability.state)}</strong> · ${esc(a.capability.mechanism)} · ${esc(a.capability.evidence)}</p><dl>${STATES.map(k => `<dt>${esc(k)}</dt><dd><strong>${esc(a[k].state)}</strong> · ${esc(a[k].mechanism)} · ${esc(a[k].evidence)}</dd>`).join('')}</dl></article>`).join('');
  root.innerHTML = `<section aria-label="Agent adapters"><header><h2>Agent adapters</h2><small>read-only projection · schema: ${esc(vm.schemaVersion)}</small></header>${rows || '<p>unknown — no adapter evidence</p>'}</section>`;
}
