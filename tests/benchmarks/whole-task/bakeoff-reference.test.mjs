import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { BAKEOFF_REFERENCE, verifyBakeoffReference } from './bakeoff-reference.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REAL_WORKSPACE_ROOT = resolve(HERE, '../..');

test('verifies against the real, already-completed bakeoff artifacts on this tree (never spawns a process)', () => {
  const result = verifyBakeoffReference({ workspaceRoot: REAL_WORKSPACE_ROOT });
  assert.equal(result.status, 'verified');
  assert.ok(result.arms.length >= 2, 'bakeoff must have compared at least two arms');
  assert.equal(result.declared.configSha256, BAKEOFF_REFERENCE.artifacts.config.sha256, 'declared configSha256 in the host receipts must equal the pinned config artifact hash — proof both hosts ran the identical, un-rerun config');
});

test('both host receipts are present and agree on config/input/manifest identity', () => {
  const result = verifyBakeoffReference({ workspaceRoot: REAL_WORKSPACE_ROOT });
  const macChecked = result.checks.find((c) => c.label === 'hosts.macos');
  const winChecked = result.checks.find((c) => c.label === 'hosts.windows');
  assert.equal(macChecked.status, 'unchanged');
  assert.equal(winChecked.status, 'unchanged');
  assert.notEqual(macChecked.actual, winChecked.actual, 'the two host receipts are distinct files (different hosts), not the same file counted twice');
});

test('fails closed when a reference artifact is missing at the given workspace root', () => {
  const emptyRoot = mkdtempSync(join(tmpdir(), 'mbr804-bakeoff-empty-'));
  try {
    const result = verifyBakeoffReference({ workspaceRoot: emptyRoot });
    assert.equal(result.status, 'missing');
  } finally {
    rmSync(emptyRoot, { recursive: true, force: true });
  }
});

test('fails closed when a reference artifact has been mutated since it was pinned (proves the check actually detects drift/rerun)', () => {
  const root = mkdtempSync(join(tmpdir(), 'mbr804-bakeoff-mutated-'));
  try {
    for (const key of ['config', 'input', 'receipt']) {
      const rel = BAKEOFF_REFERENCE.artifacts[key].path;
      mkdirSync(dirname(join(root, rel)), { recursive: true });
      writeFileSync(join(root, rel), readFileSync(resolve(REAL_WORKSPACE_ROOT, rel)));
    }
    for (const key of ['macos', 'windows']) {
      const rel = BAKEOFF_REFERENCE.hosts[key].path;
      mkdirSync(dirname(join(root, rel)), { recursive: true });
      writeFileSync(join(root, rel), readFileSync(resolve(REAL_WORKSPACE_ROOT, rel)));
    }
    // Mutate the copied config file, as a rerun of the bakeoff would.
    writeFileSync(join(root, BAKEOFF_REFERENCE.artifacts.config.path), 'tampered-not-the-real-immutable-config');

    const result = verifyBakeoffReference({ workspaceRoot: root });
    assert.equal(result.status, 'mutated');
    assert.match(result.reason, /config/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
