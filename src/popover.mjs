export const STATUS_ORDER = ['unavailable', 'degraded', 'available'];
export const SECTION_ORDER = ['deliveries', 'providers', 'repositories', 'adapters', 'devices', 'memory', 'sentinel', 'alerts'];
const label = s => ({ unavailable: 'Unavailable', degraded: 'Degraded', available: 'Available' }[s] || 'Unknown');
const severity = s => STATUS_ORDER.indexOf(s) >= 0 ? STATUS_ORDER.indexOf(s) : STATUS_ORDER.length;
const state = section => typeof section === 'object' && section ? section.state : null;
const worst = sections => {
  const states = sections.map(state);
  return states.every(value => STATUS_ORDER.includes(value)) ? states.reduce((worst, current) => severity(current) < severity(worst) ? current : worst) : null;
};
export function viewModel(snapshot) {
  const p = snapshot?.payload || {};
  const sections = SECTION_ORDER.map(key => p[key]);
  const overallState = worst(sections);
  const reason = overallState ? sections.find(section => state(section) === overallState)?.reason || 'No reason provided' : 'No cached snapshot';
  const stateText = value => label(state(value));
  const observed = snapshot?.observed_at_unix_ms ?? p.observedAtUnixMs ?? null;
  const stale = Boolean(Number(snapshot?.cache_age_ms) > 0 || sections.some(section => Number(section?.cacheAgeMs) > 0));
  return { overall: overallState ? label(overallState) : 'Offline', reason: String(reason), delivery: stateText(p.deliveries), sources: label(worst([p.providers, p.repositories])), fleet: label(worst([p.adapters, p.devices])), traceId: typeof p.traceId === 'string' && p.traceId ? p.traceId : null, observed, stale };
}
export function diagnostics(vm) { return JSON.stringify({ overall: vm.overall, delivery: vm.delivery, sources: vm.sources, fleet: vm.fleet, traceAvailable: Boolean(vm.traceId) }); }

if (typeof document !== 'undefined') (async () => {
  const [{ invoke }, { listen }, { getCurrentWindow }, { createWindowControlLabels }] = await Promise.all([import('../vendor/@tauri-apps/api/core.js'), import('../vendor/@tauri-apps/api/event.js'), import('../vendor/@tauri-apps/api/window.js'), import('../vendor/@rightkit/platform-ui/index.js')]);
  const current = getCurrentWindow(); const $ = id => document.getElementById(id);
  const windowLabels = createWindowControlLabels({ close: 'Close status panel' });
  function render(snapshot) { const vm = viewModel(snapshot); document.body.dataset.status=vm.overall.toLowerCase(); $('overall').textContent=vm.overall; $('reason').textContent=vm.reason; $('delivery').textContent=vm.delivery; $('sources').textContent=vm.sources; $('fleet').textContent=vm.fleet; $('observed').textContent=vm.observed ? `Observed ${new Date(vm.observed).toISOString()}${vm.stale ? ' · cached/stale' : ''}` : (vm.stale ? 'Cached/stale' : ''); $('trace').disabled=!vm.traceId; $('trace').dataset.traceId=vm.traceId||''; $('announce').textContent=`${vm.overall}. ${vm.reason}`; }
  async function refresh() { try { render(await invoke('snapshot')); } catch { render(null); } }
  const hide=()=>invoke('hide_popover');
  $('close').setAttribute('aria-label', windowLabels.close); $('close').onclick=hide; $('quit').onclick=()=>invoke('quit_app');
  $('diagnostics').onclick=async()=>{ try { await navigator.clipboard?.writeText(diagnostics(viewModel(await invoke('snapshot')))); $('announce').textContent='Diagnostics copied'; } catch { $('announce').textContent='Diagnostics unavailable'; } };
  $('trace').onclick=async()=>{ const id=$('trace').dataset.traceId; if(id) { try { await navigator.clipboard?.writeText(id); $('announce').textContent='Latest trace copied'; } catch { $('announce').textContent='Trace unavailable'; } } };
  listen('popover-diagnostics', ()=>$('diagnostics').click()); listen('popover-trace', ()=>$('trace').click());
  window.addEventListener('keydown', e=>{ if(e.key==='Escape') hide(); });
  current.onFocusChanged(({payload})=>{ if(!payload) hide(); });
  refresh(); listen('hub-snapshot-tick', refresh);
})();
