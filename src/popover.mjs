export const STATUS_ORDER = ['unavailable', 'degraded', 'available'];
const label = s => ({ unavailable: 'Unavailable', degraded: 'Degraded', available: 'Available' }[s] || 'Unknown');
const severity = s => STATUS_ORDER.indexOf(s) >= 0 ? STATUS_ORDER.indexOf(s) : STATUS_ORDER.length;
export function viewModel(snapshot) {
  const p = snapshot?.payload || {};
  const sections = Object.values(p).filter(v => (v && typeof v === 'object' && STATUS_ORDER.includes(v.state)) || STATUS_ORDER.includes(v));
  const status = v => typeof v === 'string' ? v : v?.state;
  const overall = sections.reduce((best, v) => severity(status(v)) < severity(best) ? status(v) : best, 'available');
  const explicitOverall = status(p.overall);
  // Aggregate only states that are explicitly present; never turn missing data into healthy.
  const overallState = explicitOverall ? overall : null;
  const reason = p.reason || p.topReason || sections.find(v => status(v) === overallState)?.reason || 'No reason provided';
  const delivery = p.lastDelivery || p.delivery || p.deliveries;
  const sources = p.sources || p.sourceReadiness || p.source_readiness;
  const fleet = p.fleet;
  const stateText = v => label(status(v));
  const observed = snapshot?.observed_at_unix_ms || p.observedAtUnixMs || null;
  const stale = Boolean(p.stale || p.cached || Number(p.cacheAgeMs) > 0 || Number(snapshot?.cache_age_ms) > 0 || sections.some(v => v && typeof v === 'object' && (v.stale === true || v.cached === true || Number(v.cacheAgeMs) > 0)));
  return { overall: overallState ? label(overallState) : 'Offline', reason: overallState ? String(reason) : 'No cached snapshot', delivery: stateText(delivery), sources: stateText(sources), fleet: stateText(fleet), traceId: typeof p.traceId === 'string' && p.traceId ? p.traceId : typeof p.trace_id === 'string' && p.trace_id ? p.trace_id : null, observed, stale };
}
export function diagnostics(vm) { return JSON.stringify({ overall: vm.overall, delivery: vm.delivery, sources: vm.sources, fleet: vm.fleet, traceAvailable: Boolean(vm.traceId) }); }

if (typeof document !== 'undefined') (async () => {
  const [{ invoke }, { listen }, { getCurrentWindow }] = await Promise.all([import('../vendor/@tauri-apps/api/core.js'), import('../vendor/@tauri-apps/api/event.js'), import('../vendor/@tauri-apps/api/window.js')]);
  const current = getCurrentWindow(); const $ = id => document.getElementById(id);
  function render(snapshot) { const vm = viewModel(snapshot); document.body.dataset.status=vm.overall.toLowerCase(); $('overall').textContent=vm.overall; $('reason').textContent=vm.reason; $('delivery').textContent=vm.delivery; $('sources').textContent=vm.sources; $('fleet').textContent=vm.fleet; $('observed').textContent=vm.observed ? `Observed ${new Date(vm.observed).toISOString()}${vm.stale ? ' · cached/stale' : ''}` : (vm.stale ? 'Cached/stale' : ''); $('trace').disabled=!vm.traceId; $('trace').dataset.traceId=vm.traceId||''; $('announce').textContent=`${vm.overall}. ${vm.reason}`; }
  async function refresh() { try { render(await invoke('snapshot')); } catch { render(null); } }
  const hide=()=>invoke('hide_popover');
  $('close').onclick=hide; $('quit').onclick=()=>invoke('quit_app');
  $('diagnostics').onclick=async()=>{ try { await navigator.clipboard?.writeText(diagnostics(viewModel(await invoke('snapshot')))); $('announce').textContent='Diagnostics copied'; } catch { $('announce').textContent='Diagnostics unavailable'; } };
  $('trace').onclick=async()=>{ const id=$('trace').dataset.traceId; if(id) { try { await navigator.clipboard?.writeText(id); $('announce').textContent='Latest trace copied'; } catch { $('announce').textContent='Trace unavailable'; } } };
  listen('popover-diagnostics', ()=>$('diagnostics').click()); listen('popover-trace', ()=>$('trace').click());
  window.addEventListener('keydown', e=>{ if(e.key==='Escape') hide(); });
  current.onFocusChanged(({payload})=>{ if(!payload) hide(); else $('close').focus(); });
  refresh(); listen('hub-snapshot-tick', refresh);
})();
