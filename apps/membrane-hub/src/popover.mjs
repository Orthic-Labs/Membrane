export const STATUS_ORDER = ['unavailable', 'degraded', 'available'];
export const SECTION_ORDER = ['deliveries', 'providers', 'repositories', 'adapters', 'devices', 'memory', 'sentinel', 'alerts'];
const label = s => ({ unavailable: 'Unavailable', degraded: 'Degraded', available: 'Available', not_configured: 'Not configured', running: 'Running', offline: 'Offline' }[s] || 'Unavailable');
const state = section => typeof section === 'object' && section ? section.state : null;
const sectionStatus = section => {
  const s = state(section);
  if (s === 'unavailable' && section.reason === 'not_instrumented') return 'not_configured';
  return STATUS_ORDER.includes(s) ? s : 'unavailable';
};
const serviceReason = (snapshot, runtime, status) => {
  if (status === 'offline') return String(runtime?.lastReason ?? runtime?.reason ?? 'Resident service offline');
  if (!snapshot) return 'No snapshot available';
  return String(runtime?.lastReason ?? runtime?.reason ?? (status === 'running' ? 'Resident service healthy' : 'Resident service degraded'));
};
function serviceState(snapshot, runtime) {
  const service = String(runtime?.serviceState ?? runtime?.service_state ?? 'unknown').toLowerCase();
  if (service === 'degraded') return 'degraded';
  if (service !== 'running') return 'offline';
  if (!snapshot) return 'degraded';
  const snapshotState = String(runtime?.snapshotState ?? runtime?.snapshot_state ?? 'unknown').toLowerCase();
  return snapshotState === 'available' || snapshotState === 'live' ? 'running' : 'degraded';
}
export function viewModel(snapshot, runtime) {
  const p = snapshot?.payload || {};
  const sectionMap = p.sections && typeof p.sections === 'object' ? p.sections : {};
  const service = serviceState(snapshot, runtime);
  const resources = Object.fromEntries(SECTION_ORDER.map(key => {
    const section = sectionMap[key];
    return [key, { state: state(section), status: label(sectionStatus(section)), reason: String(section?.reason ?? 'No evidence') }];
  }));
  const observed = snapshot?.observed_at_unix_ms ?? p.observedAtUnixMs ?? null;
  const stale = Boolean(Number(snapshot?.cache_age_ms) > 0 || SECTION_ORDER.some(key => Number(sectionMap[key]?.cacheAgeMs) > 0));
  const status = label(service);
  return {
    overall: status,
    membrane: status,
    service: { state: service, status, reason: serviceReason(snapshot, runtime, service) },
    reason: serviceReason(snapshot, runtime, service),
    resources,
    // Keep compact popover aliases, but each one now names one resource. No
    // child status can promote or demote Membrane's resident-service status.
    delivery: resources.deliveries.status,
    providers: resources.providers.status,
    blueprint: resources.repositories.status,
    sources: resources.providers.status,
    fleet: resources.adapters.status,
    devices: resources.devices.status,
    alerts: resources.alerts.status,
    traceId: typeof p.traceId === 'string' && p.traceId ? p.traceId : null,
    observed,
    stale,
  };
}
export function diagnostics(vm) { return JSON.stringify({ overall: vm.overall, service: vm.service, resources: vm.resources, traceAvailable: Boolean(vm.traceId) }); }

if (typeof document !== 'undefined') (async () => {
  const [{ invoke }, { listen }, { getCurrentWindow }, { createWindowControlLabels }] = await Promise.all([import('../vendor/@tauri-apps/api/core.js'), import('../vendor/@tauri-apps/api/event.js'), import('../vendor/@tauri-apps/api/window.js'), import('../vendor/@rightkit/platform-ui/index.js')]);
  const current = getCurrentWindow(); const $ = id => document.getElementById(id);
  const windowLabels = createWindowControlLabels({ close: 'Close status panel' });
  function render(snapshot, runtime) { const vm = viewModel(snapshot, runtime); document.body.dataset.status=vm.service.state === 'running' ? 'available' : vm.service.state; $('overall').textContent=vm.overall; $('reason').textContent=vm.reason; $('delivery').textContent=`Deliveries ${vm.delivery}`; $('sources').textContent=`Providers ${vm.providers} · Blueprint ${vm.blueprint}`; $('fleet').textContent=`Adapters ${vm.fleet} · Devices ${vm.devices} · Alerts ${vm.alerts}`; $('observed').textContent=vm.observed ? `Observed ${new Date(vm.observed).toISOString()}${vm.stale ? ' · cached/stale' : ''}` : (vm.stale ? 'Cached/stale' : ''); $('trace').disabled=!vm.traceId; $('trace').dataset.traceId=vm.traceId||''; $('announce').textContent=`Membrane ${vm.overall}. Providers ${vm.providers}. Blueprint ${vm.blueprint}.`; }
  async function refresh() { const [runtime, snapshot] = await Promise.allSettled([invoke('diagnostics_report'), invoke('snapshot')]); render(snapshot.status === 'fulfilled' ? snapshot.value : null, runtime.status === 'fulfilled' ? runtime.value : null); }
  const hide=()=>invoke('hide_popover');
  $('close').setAttribute('aria-label', windowLabels.close); $('close').onclick=hide; $('open-hub').onclick=()=>invoke('open_dashboard'); $('quit').onclick=()=>invoke('quit_app');
  $('diagnostics').onclick=async()=>{ try { const [runtime,snapshot]=await Promise.all([invoke('diagnostics_report'),invoke('snapshot')]); await navigator.clipboard?.writeText(JSON.stringify({runtime,presentation:JSON.parse(diagnostics(viewModel(snapshot,runtime)))})); $('announce').textContent='Diagnostics copied'; } catch { $('announce').textContent='Diagnostics unavailable'; } };
  $('trace').onclick=async()=>{ const id=$('trace').dataset.traceId; if(id) { try { await navigator.clipboard?.writeText(id); $('announce').textContent='Latest trace copied'; } catch { $('announce').textContent='Trace unavailable'; } } };
  listen('popover-diagnostics', ()=>$('diagnostics').click()); listen('popover-trace', ()=>$('trace').click());
  window.addEventListener('keydown', e=>{ if(e.key==='Escape') hide(); });
  current.onFocusChanged(({payload})=>{ if(!payload) hide(); });
  refresh(); listen('hub-snapshot-tick', refresh);
})();
