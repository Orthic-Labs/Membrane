// End-to-end chain acceptance: the EXACT serialized payload produced by
// `membrane cli hub-snapshot` (golden fixtures regenerated and pinned by
// engine/crates/membrane-runtime/tests/hub_snapshot_cli_contract.rs) flows
// through the Hub JS read model (overview dashboard + tray popover) and must
// render the canonical status model without inventing health.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dashboardModel, renderOverview } from '../src/overview.mjs';
import { SUBSYSTEM_ORDER, viewModel, diagnostics } from '../src/popover.mjs';

const fixture = name => readFile(new URL(`./fixtures/${name}`, import.meta.url), 'utf8').then(JSON.parse);

const freshRuntime = { serviceState: 'running', snapshotState: 'available', lastReason: 'ok' };

test('cli hub-snapshot fixture (healthy resident) renders Running parent with local child states', async () => {
  const cached = { schema_version: 1, observed_at_unix_ms: 42, payload: await fixture('hub-snapshot-running.json') };
  const vm = viewModel(cached, freshRuntime);
  // CASE 1 + CASE 6 shape: Membrane Running while Blueprint transport is unavailable.
  assert.equal(vm.overall, 'Running');
  assert.equal(vm.membrane, 'Running');
  assert.equal(vm.subsystems.blueprint.status, 'Transport unavailable');
  assert.equal(vm.subsystems.blueprint.state, 'unavailable');
  // Typed Not configured arrives natively — no reason-string reconstruction.
  for (const name of ['pull', 'push', 'ledger', 'adapt']) {
    assert.equal(vm.subsystems[name].status, 'Not configured', `${name} must be Not configured`);
    assert.equal(vm.subsystems[name].state, 'not_configured');
  }
  // Cortex owns memory+sentinel; Ledger stays distinct.
  assert.notEqual(vm.subsystems.ledger.status, vm.subsystems.cortex.status);
  // Eight operational resources remain separate surfaces.
  for (const resource of ['deliveries', 'providers', 'repositories', 'adapters', 'devices', 'memory', 'sentinel', 'alerts']) {
    assert.ok(vm.resources[resource], `resource ${resource} present`);
  }

  const root = { innerHTML: '' };
  renderOverview(cached.payload, root, freshRuntime);
  assert.match(root.innerHTML, /<h1>Membrane Running<\/h1>/);
});

test('cli hub-snapshot fixture (health.ok=false) degrades Membrane in header AND popover, not children only', async () => {
  const cached = { schema_version: 1, observed_at_unix_ms: 42, payload: await fixture('hub-snapshot-degraded.json') };
  // The producer froze membraneState=degraded; the live poll is healthy, so
  // the frozen payload state is trusted and both surfaces show Degraded.
  const vm = viewModel(cached, freshRuntime);
  assert.equal(vm.overall, 'Degraded');
  const root = { innerHTML: '' };
  renderOverview(cached.payload, root, freshRuntime);
  assert.match(root.innerHTML, /<h1>Membrane Degraded<\/h1>/);
  assert.equal(diagnostics(vm).includes('degraded'), true);
});

test('cached Running snapshot never masks current live-fetch failure (stale-cache regression)', async () => {
  const runningPayload = await fixture('hub-snapshot-running.json');
  const cachedRunning = { schema_version: 1, observed_at_unix_ms: 42, payload: runningPayload };
  // Poll now fails to obtain a live snapshot (telemetry snapshotState degraded
  // = served-from-cache) while the resident itself remains healthy.
  const cacheServedRuntime = { serviceState: 'running', snapshotState: 'degraded', lastReason: 'cached_snapshot' };
  const vm = viewModel(cachedRunning, cacheServedRuntime);
  assert.equal(vm.overall, 'Degraded', 'cached Running must degrade when the live snapshot is unavailable');
  assert.equal(vm.reason, 'cached_snapshot');

  const model = dashboardModel(runningPayload, cacheServedRuntime);
  assert.equal(model.serviceStatus, 'Degraded');
  const root = { innerHTML: '' };
  renderOverview(runningPayload, root, cacheServedRuntime);
  assert.match(root.innerHTML, /<h1>Membrane Degraded<\/h1>/);
  // Sidebar pill agrees with the header.
  assert.match(root.innerHTML, /state-degraded/);
});

test('resident loss overrides any cached snapshot state to Offline', async () => {
  const runningPayload = await fixture('hub-snapshot-running.json');
  const offlineRuntime = { serviceState: 'crash_loop', snapshotState: 'available', lastReason: 'supervisor_restart' };
  const vm = viewModel({ schema_version: 1, observed_at_unix_ms: 42, payload: runningPayload }, offlineRuntime);
  assert.equal(vm.overall, 'Offline');
  const model = dashboardModel(runningPayload, offlineRuntime);
  assert.equal(model.serviceStatus, 'Offline');
});

test('fresh live fetch restores trust in producer-frozen state (Blueprint recovery needs no parent change)', async () => {
  const runningPayload = await fixture('hub-snapshot-running.json');
  const recovered = structuredClone(runningPayload);
  recovered.subsystems.blueprint = {
    state: 'available',
    reason: 'observed',
    items: [{ graphState: 'fresh', generationId: 'gen-2' }],
    evidence: 'Blueprint status IPC',
    observedAtUnixMs: 42,
  };
  const vm = viewModel({ schema_version: 1, observed_at_unix_ms: 42, payload: recovered }, freshRuntime);
  // CASE 6: Blueprint Available, parent unchanged at Running.
  assert.equal(vm.overall, 'Running');
  assert.equal(vm.subsystems.blueprint.status, 'Available');
});

test('tray popover preserves typed Blueprint root_not_enrolled reporting', () => {
  const payload = {
    sections: { repositories: { state: 'unavailable', reason: 'root_not_enrolled' } },
    subsystems: { blueprint: { state: 'unavailable', reason: 'root_not_enrolled' } },
  };
  const vm = viewModel({ schema_version: 1, payload }, freshRuntime);
  assert.equal(vm.resources.repositories.status, 'Root not enrolled');
  assert.equal(vm.resources.repositories.reasonLabel, 'Root not enrolled');
  assert.equal(vm.subsystems.blueprint.status, 'Root not enrolled');
  assert.equal(vm.subsystems.blueprint.reasonLabel, 'Root not enrolled');
});
