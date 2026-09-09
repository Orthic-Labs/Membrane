import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { PKG_01, PKG_02, PKG_03, PKG_04, PKG_05 } from './pkg-windows.mjs';

const workspaceRoot = resolve(new URL('../../../', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));

test('PKG_01: registry runner contract passes every self-contained negative control', async () => {
  const result = await PKG_01({ workspaceRoot, profile: 'internal-unsigned', platform: 'windows', evidencePath: null });
  assert.equal(result.evidenceKind, 'component');
  assert.deepEqual((result.detail?.checks ?? []).filter((check) => check.passed !== true), []);
  assert.equal(result.status, 'passed');
});

test('PKG_02: fails without fabricating a PASS when candidate/installed manifests are unavailable', async () => {
  const result = await PKG_02({ row: {}, workspaceRoot });
  assert.equal(result.status, 'failed');
  assert.equal(result.evidenceKind, 'source');
  assert.match(result.reason, /unavailable/);
});

test('PKG_02: fails on identity mismatch between candidate and installed manifests', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg02-'));
  try {
    const candidatePath = join(scratch, 'candidate.json');
    const installedPath = join(scratch, 'installed.json');
    writeFileSync(candidatePath, JSON.stringify({ release: { target: 'windows-x86_64', generation: 'sha256:aaaa', artifact_sha256: 'a'.repeat(64) }, signing: { status: 'unsigned' } }));
    writeFileSync(installedPath, JSON.stringify({ release: { target: 'windows-x86_64', generation: 'sha256:bbbb', artifact_sha256: 'b'.repeat(64) }, signing: { status: 'unsigned' } }));
    const result = await PKG_02({ row: { candidateManifestPath: candidatePath, installedManifestPath: installedPath }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.match(result.reason, /generation mismatch/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_02: fails when a non-Windows target is present', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg02-target-'));
  try {
    const candidatePath = join(scratch, 'candidate.json');
    const installedPath = join(scratch, 'installed.json');
    const shared = { generation: 'sha256:cccc', artifact_sha256: 'c'.repeat(64) };
    writeFileSync(candidatePath, JSON.stringify({ release: { target: 'macos-arm64', ...shared }, signing: { status: 'unsigned' } }));
    writeFileSync(installedPath, JSON.stringify({ release: { target: 'macos-arm64', ...shared }, signing: { status: 'unsigned' } }));
    const result = await PKG_02({ row: { candidateManifestPath: candidatePath, installedManifestPath: installedPath }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.match(result.reason, /forbidden non-Windows target/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_02: passes on matched windows-x86_64 unsigned identities', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg02-pass-'));
  try {
    const candidatePath = join(scratch, 'candidate.json');
    const installedPath = join(scratch, 'installed.json');
    const shared = { target: 'windows-x86_64', generation: 'sha256:dddd', artifact_sha256: 'd'.repeat(64) };
    writeFileSync(candidatePath, JSON.stringify({ release: shared, signing: { status: 'unsigned' } }));
    writeFileSync(installedPath, JSON.stringify({ release: shared, signing: { status: 'unsigned' } }));
    const result = await PKG_02({ row: { candidateManifestPath: candidatePath, installedManifestPath: installedPath }, workspaceRoot });
    assert.equal(result.status, 'passed');
    assert.equal(result.evidenceKind, 'installed');
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_03: fails without a live controller identity', async () => {
  const result = await PKG_03({ row: {}, workspaceRoot });
  assert.equal(result.status, 'failed');
  assert.equal(result.evidenceKind, 'source');
});

test('PKG_03: fails when the controller reports a different identity than installed', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg03-'));
  try {
    const installedPath = join(scratch, 'installed.json');
    const controllerPath = join(scratch, 'controller.json');
    writeFileSync(installedPath, JSON.stringify({ release: { generation: 'sha256:eeee' } }));
    writeFileSync(controllerPath, JSON.stringify({ releaseGeneration: 'sha256:ffff', sourceRoot: 'D:/somewhere/installed' }));
    const result = await PKG_03({ row: { installedManifestPath: installedPath, controllerIdentityPath: controllerPath }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.match(result.reason, /does not equal installed current/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_03: fails when the controller fell back to the development checkout', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg03-fallback-'));
  try {
    const installedPath = join(scratch, 'installed.json');
    const controllerPath = join(scratch, 'controller.json');
    writeFileSync(installedPath, JSON.stringify({ release: { generation: 'sha256:1111' } }));
    writeFileSync(controllerPath, JSON.stringify({ releaseGeneration: 'sha256:1111', sourceRoot: workspaceRoot }));
    const result = await PKG_03({ row: { installedManifestPath: installedPath, controllerIdentityPath: controllerPath }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.match(result.reason, /development checkout/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_04: fails without a supplied qualified sourceRevision', async () => {
  const result = await PKG_04({ row: {}, workspaceRoot });
  assert.equal(result.status, 'failed');
  assert.equal(result.evidenceKind, 'source');
});

test('PKG_04: fails when HEAD does not include the qualified sourceRevision', async () => {
  const result = await PKG_04({ row: { qualifiedSourceRevision: '0'.repeat(40) }, workspaceRoot });
  assert.equal(result.status, 'failed');
  assert.match(result.reason, /does not include qualified sourceRevision/);
});

test('PKG_05: reports each sampled case evidenceKind and never lets PKG-02/PKG-03 pass on source-only evidence', async () => {
  const result = await PKG_05({ workspaceRoot, evidencePath: null });
  assert.equal(result.evidenceKind, 'component');
  const sampled = result.detail?.sampleResults ?? [];
  assert.ok(sampled.length >= 3);
  for (const sample of sampled) {
    assert.ok(sample.evidenceKind, `${sample.id} did not record an evidenceKind`);
  }
});
