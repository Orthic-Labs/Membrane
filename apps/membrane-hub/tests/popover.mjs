import test from 'node:test'; import assert from 'node:assert/strict';
import { SECTION_ORDER, viewModel, diagnostics } from '../src/popover.mjs';

const section = (state = 'available', reason = `${state}_reason`) => ({ state, reason, items: [], resolver: null, source: null, evidence: null, observedAtUnixMs: 1, cacheAgeMs: 0 });
const golden = (overrides = {}, service = { state: 'running', reason: 'ok', ok: true }) => ({ schemaVersion: 1, observedAtUnixMs: 1, service, sections: { ...Object.fromEntries(SECTION_ORDER.map(key => [key, section()])), ...overrides } });
const running = snapshot => viewModel(snapshot, { serviceState: 'running', snapshotState: 'available', lastReason: 'ok' });

test('canonical snapshot keeps exactly eight independent resource sections', () => {
  const vm = running({ payload: golden({ providers: section('degraded', 'daily_analysis_stale'), repositories: section('unavailable', 'blueprint_unavailable'), devices: section('unavailable', 'not_instrumented'), alerts: section('unavailable', 'not_instrumented') }) });
  assert.deepEqual(Object.keys(vm.resources), SECTION_ORDER);
  assert.equal(vm.overall, 'Running'); assert.equal(vm.providers, 'Degraded'); assert.equal(vm.blueprint, 'Unavailable');
  assert.equal(vm.devices, 'Not configured'); assert.equal(vm.alerts, 'Not configured');
});

test('child failure never promotes or demotes resident Membrane status', () => {
  const vm = running({ payload: golden({ providers: section('unavailable', 'provider_failed'), repositories: section('unavailable', 'blueprint_unavailable') }) });
  assert.equal(vm.overall, 'Running'); assert.equal(vm.reason, 'ok'); assert.equal(vm.sources, 'Unavailable');
});

test('resident health drives Membrane status independently of resources', () => {
  const degraded = viewModel({ payload: golden() }, { serviceState: 'degraded', snapshotState: 'available', lastReason: 'health_failed' });
  assert.equal(degraded.overall, 'Degraded'); assert.equal(degraded.reason, 'health_failed');
  assert.equal(viewModel(null, { serviceState: 'running', snapshotState: 'unavailable' }).overall, 'Degraded');
  const offline = viewModel(null); assert.equal(offline.overall, 'Offline'); assert.equal(offline.reason, 'Resident service offline');
});

test('fresh Blueprint remains local while Membrane remains Running', () => {
  const vm = running({ payload: golden({ repositories: section('available', 'graph_fresh') }) });
  assert.equal(vm.overall, 'Running'); assert.equal(vm.blueprint, 'Available');
});

test('cached Running payload never masks a failed live fetch while resident stays healthy', () => {
  // Previous cycle froze membraneState=running; this cycle the poll served
  // cache (snapshotState=degraded). Current observation wins: Degraded.
  const cachedRunning = { payload: { ...golden(), membraneState: 'running' } };
  const vm = viewModel(cachedRunning, { serviceState: 'running', snapshotState: 'degraded', lastReason: 'cached_snapshot' });
  assert.equal(vm.overall, 'Degraded');
  assert.equal(vm.service.state, 'degraded');
  assert.equal(vm.reason, 'cached_snapshot');
});

test('resident loss promotes Offline regardless of any cached parent state', () => {
  const cachedRunning = { payload: { ...golden(), membrane_state: 'running' } };
  const vm = viewModel(cachedRunning, { serviceState: 'unavailable', snapshotState: 'available', lastReason: 'supervisor_lost' });
  assert.equal(vm.overall, 'Offline');
});

test('typed subsystem contract carries Not configured natively without reason decoding', () => {
  const vm = running({ payload: { ...golden(), subsystems: { pull: { state: 'not_configured', reason: 'no_producer' }, blueprint: { state: 'unavailable', reason: 'blueprint_unavailable' }, cortex: { state: 'available', reason: 'observed' } } } });
  assert.equal(vm.subsystems.pull.status, 'Not configured');
  assert.equal(vm.subsystems.pull.state, 'not_configured');
  assert.equal(vm.subsystems.blueprint.status, 'Unavailable');
  assert.equal(vm.subsystems.cortex.status, 'Available');
  assert.equal(vm.overall, 'Running', 'child states stay local');
});

test('not_instrumented is presented as Not configured, never Degraded', () => {
  const vm = running({ payload: golden({ devices: section('unavailable', 'not_instrumented'), alerts: section('unavailable', 'not_instrumented') }) });
  assert.equal(vm.resources.devices.status, 'Not configured'); assert.equal(vm.resources.alerts.status, 'Not configured'); assert.equal(vm.overall, 'Running');
});

test('all resource states remain local, including invalid or missing sections', () => {
  const vm = running({ payload: golden({ memory: undefined, sentinel: undefined, devices: true, adapters: 42 }) });
  assert.equal(vm.overall, 'Running'); assert.equal(vm.resources.memory.status, 'Unavailable'); assert.equal(vm.resources.sentinel.status, 'Unavailable');
  assert.equal(vm.resources.devices.status, 'Unavailable'); assert.equal(vm.resources.adapters.status, 'Unavailable');
});

test('trace is unavailable without explicit id and diagnostics contain no payload', () => {
  const vm = running({ payload: { sections: { devices: section('unavailable') }, secret: 'nope' } });
  assert.equal(vm.traceId, null); assert.doesNotMatch(diagnostics(vm), /nope/); assert.match(diagnostics(vm), /resources/);
});

test('popover rendering names resident service, Blueprint, providers, and configured gaps', async () => {
  const fs = await import('node:fs/promises');
  const [html, css, popover] = await Promise.all([fs.readFile(new URL('../popover.html', import.meta.url), 'utf8'), fs.readFile(new URL('../src/popover.css', import.meta.url), 'utf8'), fs.readFile(new URL('../src/popover.mjs', import.meta.url), 'utf8')]);
  assert.match(html, /id="open-hub">Open Hub/); assert.match(html, /brand-mark/); assert.match(html, /id="close"/); assert.match(html, /Quit Membrane/);
  assert.match(popover, /Membrane \$\{vm\.overall\}/); assert.match(popover, /Providers \$\{vm\.providers\}/); assert.match(popover, /Blueprint \$\{vm\.blueprint\}/); assert.match(popover, /Not configured/);
  assert.match(popover, /invoke\('open_dashboard'\)/); assert.match(popover, /vendor\/@rightkit\/platform-ui\/index\.js/); assert.match(popover, /createWindowControlLabels\(\{ close: 'Close status panel' \}\)/);
  assert.match(css, /background:#a86bff/); for (const state of ['available', 'degraded', 'unavailable']) assert.match(css, new RegExp(`data-status=${state}`));
});
