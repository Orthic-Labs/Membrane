'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { validateObservableEvent } = require('./observable-event.cjs');

const MAX_RECORD_BYTES = 256 * 1024;

// Canonical workspace runtime config (host/port identity for the membrane-local-v1 service).
// Mirrors tools/lib/memory/runtime_config.py's identity check. Read-only reference — never edited here.
const RUNTIME_CONFIG_PATH = path.join(__dirname, '..', '..', '..', 'tools', 'lib', 'memory', 'runtime.json');

// In-process cache: each hook invocation is a short-lived Node process, so this mainly protects
// call sites that invoke appendObservableEvent more than once per process. The resolution work
// itself is one small synchronous file read + one existsSync stat — negligible either way.
let cachedDefaultTarget;

function resetIngressResolutionCache() {
  cachedDefaultTarget = undefined;
}

// Test-only: force the memoized resolution result without touching the real runtime.json, so
// appendObservableEvent's "config unresolvable" branch is exercised through its real call path.
function setCachedDefaultTargetForTest(value) {
  cachedDefaultTarget = value;
}

// Validates the config file identifies itself as the real membrane-local-v1 runtime config, the same
// identity check runtime_config.py performs before trusting its contents. Throws on any mismatch;
// callers treat a throw as "config unresolvable".
function resolveRuntimeConfigIdentity(configPath) {
  const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  if (config.schemaVersion !== 1 || config.serviceId !== 'membrane-local-v1') {
    throw new Error('invalid membrane runtime config identity');
  }
  if (config.host !== '127.0.0.1') {
    throw new Error('membrane runtime host must remain loopback-only');
  }
  return config;
}

// Mirrors cortex-store/src/context_telemetry.rs::default_prompt_telemetry_ingress: the ingress
// journal lives next to the cortex db (CORTEX_DB override, else the workspace-standard cache path
// derived from the same scripts/tools/ tree runtime.json lives in). Returns { ok:true, target, dbPath }
// or { ok:false } when the config can't be trusted enough to derive a path from it.
function resolveDefaultIngressTarget(configPath = RUNTIME_CONFIG_PATH, env = process.env) {
  try {
    resolveRuntimeConfigIdentity(configPath);
    const dbPath = path.resolve(
      env.CORTEX_DB || path.join(path.dirname(configPath), '..', '..', '.cache', 'memory', 'cortex-engine.db'),
    );
    const target = path.join(path.dirname(dbPath), 'context-telemetry-ingress.jsonl');
    return { ok: true, target, dbPath };
  } catch {
    return { ok: false };
  }
}

function cachedResolveDefaultIngressTarget() {
  if (!cachedDefaultTarget) cachedDefaultTarget = resolveDefaultIngressTarget();
  return cachedDefaultTarget;
}

function appendObservableEvent(event) {
  validateObservableEvent(event);

  const explicitTarget = process.env.MEMBRANE_TELEMETRY_INGRESS;
  let target = explicitTarget;
  let requireDrainEvidence = false;

  if (!target) {
    const resolved = cachedResolveDefaultIngressTarget();
    if (!resolved.ok) return { status: 'unavailable', reason: 'telemetry_ingress_unconfigured' };
    target = resolved.target;
    // Cheap, synchronous, no network/subprocess: the resident membrane-runtime "cortex-prompt-telemetry"
    // drain thread opens db_path before it ever drains this journal, so an absent db file is a reliable,
    // negligible-cost signal that the resident service has not run — never claim persisted in that case.
    requireDrainEvidence = !fs.existsSync(resolved.dbPath);
  }

  if (requireDrainEvidence) {
    return { status: 'unavailable', reason: 'telemetry_ingress_drain_service_not_running' };
  }

  const record = Buffer.from(`${JSON.stringify({ observable_events: [event] })}\n`, 'utf8');
  if (record.length > MAX_RECORD_BYTES) return { status: 'unavailable', reason: 'telemetry_record_too_large' };
  try {
    const fd = fs.openSync(target, 'a', 0o600);
    try {
      fs.writeSync(fd, record);
      fs.fsyncSync(fd);
    } finally {
      fs.closeSync(fd);
    }
    return { status: 'persisted', target: 'membrane_prompt_ingress' };
  } catch {
    return { status: 'unavailable', reason: 'telemetry_ingress_unavailable' };
  }
}

module.exports = {
  appendObservableEvent,
  // Exported for tests only; production call sites use appendObservableEvent exclusively.
  _internal: {
    resolveDefaultIngressTarget, resolveRuntimeConfigIdentity, resetIngressResolutionCache,
    _setCachedDefaultTargetForTest: setCachedDefaultTargetForTest, RUNTIME_CONFIG_PATH,
  },
};
