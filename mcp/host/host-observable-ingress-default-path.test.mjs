import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const ingress = require('./observable-ingress.cjs');
const { appendObservableEvent, _internal } = ingress;
const { buildObservableEvent } = require('./observable-event.cjs');
const { resolveDefaultIngressTarget, resolveRuntimeConfigIdentity, resetIngressResolutionCache, RUNTIME_CONFIG_PATH } = _internal;

function sampleEvent(overrides = {}) {
  return buildObservableEvent({
    installationId: 'install-1', clientId: 'codex', sessionId: 'session-1', taskId: 'task-1', turnId: 'turn-1', traceId: 'trace-1',
    eventType: 'tool_receipt', origin: 'tool', content: 'private command output', completeness: { receipt: true },
    ...overrides,
  });
}

function withEnv(overrides, fn) {
  const previous = {};
  for (const key of Object.keys(overrides)) previous[key] = process.env[key];
  for (const [key, value] of Object.entries(overrides)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  resetIngressResolutionCache();
  try {
    return fn();
  } finally {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    resetIngressResolutionCache();
  }
}

test('the real workspace runtime.json resolves the cortex-local-v1 identity (path convention still matches production)', () => {
  const resolved = resolveDefaultIngressTarget();
  assert.equal(resolved.ok, true);
  assert.match(resolved.target, /context-telemetry-ingress\.jsonl$/);
  assert.equal(path.dirname(resolved.target), path.dirname(resolved.dbPath));
  // Matches the Rust default_prompt_telemetry_ingress convention: journal sits beside the db.
  assert.equal(path.basename(RUNTIME_CONFIG_PATH), 'runtime.json');
});

test('default path is used (and honored) when CORTEX_TELEMETRY_INGRESS is unset and the resolved db file exists', () => {
  withEnv({ CORTEX_TELEMETRY_INGRESS: undefined }, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-default-ingress-'));
    const dbPath = path.join(directory, 'cortex-engine.db');
    fs.writeFileSync(dbPath, ''); // simulate: the resident service has initialized its db
    withEnv({ CORTEX_DB: dbPath }, () => {
      const event = sampleEvent({ traceId: 'trace-default-ok' });
      const result = appendObservableEvent(event);
      assert.deepEqual(result, { status: 'persisted', target: 'membrane_prompt_ingress' });
      const expectedTarget = path.join(directory, 'context-telemetry-ingress.jsonl');
      const record = JSON.parse(fs.readFileSync(expectedTarget, 'utf8'));
      assert.deepEqual(record.observable_events[0], event);
    });
    fs.rmSync(directory, { recursive: true, force: true });
  });
});

test('resolved-but-service-not-running reports a distinct honest reason, never a false persisted', () => {
  withEnv({ CORTEX_TELEMETRY_INGRESS: undefined }, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-no-db-'));
    const dbPath = path.join(directory, 'cortex-engine.db'); // deliberately never created
    withEnv({ CORTEX_DB: dbPath }, () => {
      const result = appendObservableEvent(sampleEvent({ traceId: 'trace-no-service' }));
      assert.deepEqual(result, { status: 'unavailable', reason: 'telemetry_ingress_drain_service_not_running' });
      assert.equal(fs.existsSync(path.join(directory, 'context-telemetry-ingress.jsonl')), false);
    });
    fs.rmSync(directory, { recursive: true, force: true });
  });
});

test('explicit CORTEX_TELEMETRY_INGRESS still overrides the default path, bypassing the drain-evidence gate', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-explicit-override-'));
  const target = path.join(directory, 'events.jsonl');
  const missingDbPath = path.join(directory, 'never-created.db'); // proves override needs no drain evidence
  withEnv({ CORTEX_TELEMETRY_INGRESS: target, CORTEX_DB: missingDbPath }, () => {
    const event = sampleEvent({ traceId: 'trace-explicit' });
    assert.deepEqual(appendObservableEvent(event), { status: 'persisted', target: 'membrane_prompt_ingress' });
    const record = JSON.parse(fs.readFileSync(target, 'utf8'));
    assert.deepEqual(record.observable_events[0], event);
  });
  fs.rmSync(directory, { recursive: true, force: true });
});

test('unresolvable config (missing file) returns ok:false, never throws to the caller', () => {
  const missing = path.join(os.tmpdir(), 'forge-does-not-exist', 'runtime.json');
  assert.deepEqual(resolveDefaultIngressTarget(missing), { ok: false });
});

test('unresolvable config (malformed JSON) returns ok:false', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-bad-json-'));
  const configPath = path.join(directory, 'runtime.json');
  fs.writeFileSync(configPath, '{ not valid json');
  assert.deepEqual(resolveDefaultIngressTarget(configPath), { ok: false });
  fs.rmSync(directory, { recursive: true, force: true });
});

test('unresolvable config (wrong serviceId / schemaVersion / host identity) returns ok:false for each mismatch', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-bad-identity-'));
  const cases = [
    { schemaVersion: 1, serviceId: 'not-cortex-local-v1', host: '127.0.0.1', port: 47851 },
    { schemaVersion: 2, serviceId: 'cortex-local-v1', host: '127.0.0.1', port: 47851 },
    { schemaVersion: 1, serviceId: 'cortex-local-v1', host: '0.0.0.0', port: 47851 },
  ];
  cases.forEach((config, index) => {
    const configPath = path.join(directory, `runtime-${index}.json`);
    fs.writeFileSync(configPath, JSON.stringify(config));
    assert.deepEqual(resolveDefaultIngressTarget(configPath), { ok: false });
    assert.throws(() => resolveRuntimeConfigIdentity(configPath));
  });
  fs.rmSync(directory, { recursive: true, force: true });
});

test('unresolvable default resolution surfaces through appendObservableEvent as an honest unavailable status', () => {
  resetIngressResolutionCache();
  _internal._setCachedDefaultTargetForTest({ ok: false });
  try {
    withEnv({ CORTEX_TELEMETRY_INGRESS: undefined }, () => {
      // withEnv resets the cache too, so re-poison it after that reset.
      _internal._setCachedDefaultTargetForTest({ ok: false });
      const result = appendObservableEvent(sampleEvent({ traceId: 'trace-unresolvable' }));
      assert.deepEqual(result, { status: 'unavailable', reason: 'telemetry_ingress_unconfigured' });
    });
  } finally {
    resetIngressResolutionCache();
  }
});

test('forbidden-field validation still rejects tampered records regardless of ingress path resolution', () => {
  const event = sampleEvent({ traceId: 'trace-forbidden' });
  assert.throws(() => appendObservableEvent({ ...event, extra_field: 'nope' }), /forbidden field/);
});
