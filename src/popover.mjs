export const STATUS_ORDER = ['unavailable', 'degraded', 'available'];
const label = s => ({ unavailable: 'Unavailable', degraded: 'Degraded', available: 'Available', offline: 'Offline' }[s] || 'Unknown');
const state = section => typeof section === 'object' && section ? section.state : null;
const PRESENTATION_ORDER = ['offline', 'unavailable', 'degraded', 'available'];
const presentationStatus = section => { const s = state(section); if (!STATUS_ORDER.includes(s)) return 'offline'; if (s === 'unavailable' && section.reason === 'not_instrumented') return 'degraded'; return s; };
const pSeverity = s => { const i = PRESENTATION_ORDER.indexOf(s); return i >= 0 ? i : PRESENTATION_ORDER.length; };
const worstPresentation = sections => { let worst = 'available'; for (const section of sections) { const ps = presentationStatus(section); if (pSeverity(ps) < pSeverity(worst)) worst = ps; } return worst; };
const worstReason = sections => { const worst = worstPresentation(sections); if (worst === 'offline') return 'No cached snapshot'; return sections.find(section => presentationStatus(section) === worst)?.reason || 'No reason provided'; };
export function viewModel(snapshot) {
  const p = snapshot?.payload || {};
  // Generic: use whatever sections the snapshot declares.
  const sectionsMap = p.sections || p;
  const metaKeys = new Set(["schemaVersion","observedAtUnixMs","productId","stale","cacheAgeMs","traceId","operation","schema"]);
  let sections;
  if (p.sections && typeof p.sections === "object") {
    sections = Object.values(p.sections);
  } else {
    // Legacy flat shape: collect values that look like sections
    sections = Object.entries(p).filter(([k,v]) => !metaKeys.has(k) && v && typeof v === "object" && "state" in v).map(([,v]) => v);
    // If no sections found, fallback to empty -> offline
    if (sections.length === 0) sections = [];
  }
  const overallPS = worstPresentation(sections);
  const reason = worstReason(sections);
  const observed = snapshot?.observed_at_unix_ms ?? p.observedAtUnixMs ?? null;
  const stale = Boolean(Number(snapshot?.cache_age_ms) > 0 || sections.some(section => Number(section?.cacheAgeMs) > 0) || Number(p.cacheAgeMs ?? 0) > 0);
  // Generic fact rows: derive delivery/sources/fleet from section order, not hardcoded names.
  // Keeps Membrane's 8-section grouping (deliveries | providers+repositories | adapters+devices)
  // when sections are in canonical order, but works for any product without string-matching product vocabulary (I-1).
  // Include all keys (even invalid/raw) so malformed sections drive groups Offline, matching overall semantics.
  const sectionKeys = Object.keys(sectionsMap).filter(k => !metaKeys.has(k));
  // Fallback to p's keys for legacy flat shape (include all non-meta keys)
  const flatKeys = sectionKeys.length ? sectionKeys : Object.keys(p).filter(k => !metaKeys.has(k));
  const ordered = flatKeys.length ? flatKeys : sections.map((_, i) => `__${i}`);
  const getByKey = key => sectionsMap[key] ?? p[key];
  const groupWorstByKeys = keys => {
    // Keep falsy values like null/0? Original filtered Boolean, but null should contribute Offline via invalid state.
    // Preserve original: filter only undefined, keep null/numbers/strings so invalid still counted.
    const vals = keys.map(getByKey).filter(v => v !== undefined);
    return label(worstPresentation(vals));
  };
  const deliveryKeys = ordered.slice(0, 1);
  const sourcesKeys = ordered.slice(1, 3);
  const fleetKeys = ordered.slice(3, 5);
  // If product has fewer sections, missing groups degrade to overall rather than false Available
  const deliveryVal = deliveryKeys.length ? groupWorstByKeys(deliveryKeys) : label(overallPS);
  const sourcesVal = sourcesKeys.length ? groupWorstByKeys(sourcesKeys) : label(overallPS);
  const fleetVal = fleetKeys.length ? groupWorstByKeys(fleetKeys) : label(overallPS);
  return { overall: label(overallPS), reason: String(reason), delivery: deliveryVal, sources: sourcesVal, fleet: fleetVal, traceId: typeof p.traceId === 'string' && p.traceId ? p.traceId : null, observed, stale };
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
