import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runCli } from './psh-windows.mjs';

test('qualification CLI preserves context isolation with per-call precedence', () => {
  const result = runCli({ cliPath: process.execPath, env: { MEMBRANE_PROJECT_REGISTRY: 'isolated', PSH_ENV_TEST: 'context' } },
    ['-e', 'console.log(JSON.stringify([process.env.MEMBRANE_PROJECT_REGISTRY,process.env.PSH_ENV_TEST,Boolean(process.env.SystemRoot || process.env.HOME)]))'],
    { env: { PSH_ENV_TEST: 'call' } });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), ['isolated', 'call', true]);
});

test('installed enrollment uses fixture registry & leaves live registry unchanged', { skip: !process.env.PSH_ISOLATION_CLI }, () => {
  const root = mkdtempSync(join(tmpdir(), 'psh-isolation-'));
  const registry = join(root, 'project-registry.json');
  const live = join(process.env.APPDATA || process.env.HOME, 'Cortex', 'project-registry.json');
  const before = existsSync(live) ? readFileSync(live) : null;
  try {
    const result = runCli({ cliPath: process.env.PSH_ISOLATION_CLI, env: { MEMBRANE_PROJECT_REGISTRY: registry, MEMBRANE_WORKSPACE_ROOT: root } },
      ['init', root, '--repository', 'psh-isolation', '--scope', 'psh-isolation']);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.ok(existsSync(registry), 'fixture registry must exist');
    const data = JSON.parse(readFileSync(registry, 'utf8'));
    assert.equal(data.bindings[root].repository_id, 'psh-isolation');
  } finally {
    assert.deepEqual(existsSync(live) ? readFileSync(live) : null, before, 'live registry must remain byte-identical');
    rmSync(root, { recursive: true, force: true });
  }
});
