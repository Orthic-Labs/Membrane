import { createHash } from 'node:crypto';
import { existsSync, readdirSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import matrix from './fault-matrix.v1.json' with { type: 'json' };

const codes = new Map(matrix.scenarios.map((s) => [s.fault, s.expected]));
const digest = (value) => `sha256:${createHash('sha256').update(JSON.stringify(value)).digest('hex')}`;

const crateRoot = dirname(fileURLToPath(import.meta.url));
const canonicalBinary = resolve(crateRoot, 'target', 'debug', process.platform === 'win32' ? 'membrane-fault-injection.exe' : 'membrane-fault-injection');
// This crate depends on membrane-protocol, membrane-runtime, and cortex-store (see Cargo.toml) --
// only those crates (plus this crate's own src) can affect the built binary's behavior.
const dependencyCrateRoots = ['membrane-protocol', 'membrane-runtime', 'cortex-store']
  .map((name) => resolve(crateRoot, '..', '..', 'engine', 'crates', name));

// Walks the given directories' own sources (skipping build output and non-build-affecting
// "tests"/"examples"/"benches" targets) for the newest mtime, so an already-built
// membrane-fault-injection binary is reused instead of rebuilt when nothing relevant changed.
function newestSourceMtimeMs(dirs) {
  let newest = 0;
  const stack = [...dirs];
  while (stack.length) {
    const current = stack.pop();
    let entries;
    try { entries = readdirSync(current, { withFileTypes: true }); } catch { continue; }
    for (const entry of entries) {
      if (['target', 'tests', 'examples', 'benches'].includes(entry.name) || entry.name.startsWith('.')) continue;
      const full = join(current, entry.name);
      if (entry.isDirectory()) { stack.push(full); continue; }
      if (!entry.name.endsWith('.rs') && entry.name !== 'Cargo.toml' && entry.name !== 'Cargo.lock') continue;
      const mtime = statSync(full).mtimeMs;
      if (mtime > newest) newest = mtime;
    }
  }
  return newest;
}

// Resolves the membrane-fault-injection binary to exercise. Reuses an already-fresh build when
// possible. Test orchestration never starts Cargo implicitly: callers prebuild this target with
// direct Cargo under the workspace's long-running-command heartbeat discipline.
function resolveBinary() {
  if (process.env.MEMBRANE_FAULT_TEST_BIN) return process.env.MEMBRANE_FAULT_TEST_BIN;
  if (existsSync(canonicalBinary) && statSync(canonicalBinary).mtimeMs >= newestSourceMtimeMs([crateRoot, ...dependencyCrateRoots])) return canonicalBinary;
  return null;
}

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
  const binary = resolveBinary();
  if (!binary) throw new Error('membrane-fault-injection binary unavailable: prebuild tests/fault-injection with direct Cargo or set MEMBRANE_FAULT_TEST_BIN');
  const result = spawnSync(binary, [], { encoding: 'utf8', timeout: 120_000 });
  if (result.status !== 0) throw new Error(result.stderr || 'fault harness failed');
  return JSON.parse(result.stdout.trim());
}
export { matrix };
