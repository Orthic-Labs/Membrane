export const STATUS_ORDER = ['unavailable', 'degraded', 'available'];
export const SECTION_ORDER = ['deliveries', 'providers', 'repositories', 'adapters', 'devices', 'memory', 'sentinel', 'alerts'];
const label = s => ({ unavailable: 'Unavailable', degraded: 'Degraded', available: 'Available', offline: 'Offline' }[s] || 'Unknown');
const state = section => typeof section === 'object' && section ? section.state : null;
const PRESENTATION_ORDER = ['offline', 'unavailable', 'degraded', 'available'];
const presentationStatus = section => { const s = state(section); if (!STATUS_ORDER.includes(s)) return 'offline'; if (s === 'unavailable' && section.reason === 'not_instrumented') return 'degraded'; return s; };
const pSeverity = s => { const i = PRESENTATION_ORDER.indexOf(s); return i >= 0 ? i : PRESENTATION_ORDER.length; };
const worstPresentation = sections => { let worst = 'available'; for (const section of sections) { const ps = presentationStatus(section); if (pSeverity(ps) < pSeverity(worst)) worst = ps; } return worst; };
const worstReason = sections => { const worst = worstPresentation(sections); if (worst === 'offline') return 'No cached snapshot'; return sections.find(section => presentationStatus(section) === worst)?.reason || 'No reason provided'; };
export function viewModel(snapshot) {
  const p = snapshot?.payload || {};
  const sections = SECTION_ORDER.map(key => p[key]);
  const overallPS = worstPresentation(sections);
  const reason = worstReason(sections);
  const observed = snapshot?.observed_at_unix_ms ?? p.observedAtUnixMs ?? null;
  const stale = Boolean(Number(snapshot?.cache_age_ms) > 0 || sections.some(section => Number(section?.cacheAgeMs) > 0));
  return { overall: label(overallPS), reason: String(reason), delivery: label(worstPresentation([p.deliveries])), sources: label(worstPresentation([p.providers, p.repositories])), fleet: label(worstPresentation([p.adapters, p.devices])), traceId: typeof p.traceId === 'string' && p.traceId ? p.traceId : null, observed, stale };
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
