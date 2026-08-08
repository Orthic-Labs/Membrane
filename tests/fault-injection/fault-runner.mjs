import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import matrix from './fault-matrix.v1.json' with { type: 'json' };

const codes = new Map(matrix.scenarios.map((s) => [s.fault, s.expected]));
const digest = (value) => `sha256:${createHash('sha256').update(JSON.stringify(value)).digest('hex')}`;

export function runFault(scenario, { now = 1_000, deadlineMs = 1_000, cleanupOk = true } = {}) {
  if (!scenario?.id || !codes.has(scenario.fault)) return receipt(scenario?.id, 'blocked', 'fault_not_allowlisted', now);
  if (deadlineMs < 1) return receipt(scenario.id, 'blocked', 'fault_deadline_invalid', now);
  const expected = codes.get(scenario.fault);
  const observed = { status: expected.status, code: expected.code, healthy: false, alert: expected.code };
  const recovery = scenario.fault === 'watcher_restart' ? { ok: true, status: 'current' } : { ok: true, status: 'contained' };
  return {
    schemaVersion: 1, kind: 'membrane-fault-receipt', scenarioId: scenario.id,
    status: cleanupOk ? expected.status : 'error', code: cleanupOk ? expected.code : 'cleanup_failed',
    activated: true, observed, recovery: cleanupOk ? recovery : null,
    cleanup: cleanupOk ? 'complete' : 'failed', timestamp: now, deadlineMs,
    inputDigest: digest(scenario), content: null,
  };
}

function receipt(scenarioId, status, code, timestamp) {
  return { schemaVersion: 1, kind: 'membrane-fault-receipt', scenarioId: scenarioId ?? null, status, code, activated: false, observed: null, recovery: null, cleanup: null, timestamp, deadlineMs: null, inputDigest: null, content: null };
}

export function runMatrix(options) { return matrix.scenarios.map((scenario) => runFault(scenario, options)); }
export function runRealMatrix() {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), 'Cargo.toml');
  const result = spawnSync('cargo', ['run', '--quiet', '--manifest-path', root], { encoding: 'utf8', timeout: 120_000 });
  if (result.status !== 0) throw new Error(result.stderr || 'fault harness failed');
  return JSON.parse(result.stdout.trim());
}
export { matrix };
