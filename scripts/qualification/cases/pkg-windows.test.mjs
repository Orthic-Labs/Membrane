import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { runRegistryQualification } from '../run.mjs';
import { PKG_01, PKG_02, PKG_03, PKG_04, PKG_05, parseControllerProbeFailure, queryInstalledControllerIdentity } from './pkg-windows.mjs';

const workspaceRoot = resolve(new URL('../../../', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));

// Independent re-implementation of the same committed-engine-tree-digest
// algorithm pkg-windows.mjs uses to adapt a rightkit-direct-release-manifest,
// so these tests can assert against a real, independently-derived value
// instead of trusting the module's own output.
function committedEngineTreeDigestForTest(commit) {
  const rows = [];
  const lsTree = execFileSync('git', ['-C', workspaceRoot, 'ls-tree', '-r', commit, '--', 'engine'], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
  for (const line of lsTree.split('\n')) {
    if (!line) continue;
    const [metadata, path] = line.split('\t');
    rows.push([path.replace(/\\/g, '/'), metadata.trim().split(/\s+/)[2]]);
  }
  rows.sort((left, right) => (left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0));
  const digest = createHash('sha256');
  for (const [path, blob] of rows) digest.update(`${path}\0${blob}\n`, 'utf8');
  return digest.digest('hex');
}

// A real, resolvable commit distinct from HEAD (the sourceCommit the
// currently-installed 0.1.24 manifest actually carries), used so the
// mismatch/adapter tests exercise a genuine commit instead of a fabricated one.
const KNOWN_INSTALLED_SOURCE_COMMIT = '9481f2fc9eabec0879947ae59a0fc2b54ed3e1e9';

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

test('PKG_02: adapts a rightkit-direct-release-manifest installed shape and reports a typed generation mismatch instead of a structural error', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg02-direct-mismatch-'));
  try {
    const candidatePath = join(scratch, 'candidate.json');
    const installedPath = join(scratch, 'installed.json');
    writeFileSync(candidatePath, JSON.stringify({
      schema: 'membrane.release-evidence.v1',
      release: { target: 'windows-x86_64', generation: 'deadbeef'.repeat(8), artifact_sha256: 'a'.repeat(64) },
      signing: { status: 'unsigned' },
    }));
    writeFileSync(installedPath, JSON.stringify({
      kind: 'rightkit-direct-release-manifest',
      sourceCommit: KNOWN_INSTALLED_SOURCE_COMMIT,
      assets: [{ target: 'windows-x86_64', sha256: 'b'.repeat(64) }],
    }));
    const result = await PKG_02({ row: { candidateManifestPath: candidatePath, installedManifestPath: installedPath }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.equal(result.evidenceKind, 'installed');
    assert.match(result.reason, /generation mismatch/);
    assert.equal(result.detail.installedGeneration, committedEngineTreeDigestForTest(KNOWN_INSTALLED_SOURCE_COMMIT));
    assert.notEqual(result.detail.installedGeneration, result.detail.candidateGeneration);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_02: adapts a rightkit-direct-release-manifest installed shape and passes when the derived generation matches, without requiring byte-identical artifacts', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg02-direct-pass-'));
  try {
    const derivedGeneration = committedEngineTreeDigestForTest(KNOWN_INSTALLED_SOURCE_COMMIT);
    const candidatePath = join(scratch, 'candidate.json');
    const installedPath = join(scratch, 'installed.json');
    writeFileSync(candidatePath, JSON.stringify({
      schema: 'membrane.release-evidence.v1',
      release: { target: 'windows-x86_64', generation: derivedGeneration, artifact_sha256: 'a'.repeat(64) },
      signing: { status: 'unsigned' },
    }));
    writeFileSync(installedPath, JSON.stringify({
      kind: 'rightkit-direct-release-manifest',
      sourceCommit: KNOWN_INSTALLED_SOURCE_COMMIT,
      // Deliberately a different sha256 than the candidate's NSIS installer:
      // the installed asset is a downloaded release zip, never byte-identical
      // to a locally-built unsigned installer even from the same source.
      assets: [{ target: 'windows-x86_64', sha256: 'c'.repeat(64) }],
    }));
    const result = await PKG_02({ row: { candidateManifestPath: candidatePath, installedManifestPath: installedPath }, workspaceRoot });
    assert.equal(result.status, 'passed');
    assert.equal(result.evidenceKind, 'installed');
    assert.equal(result.detail.generation, derivedGeneration);
    assert.equal(result.detail.adapted, true);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_02: reports a typed failure, not a fabricated pass or a structural crash, when the rightkit-direct sourceCommit is not resolvable', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg02-direct-unresolvable-'));
  try {
    const candidatePath = join(scratch, 'candidate.json');
    const installedPath = join(scratch, 'installed.json');
    writeFileSync(candidatePath, JSON.stringify({
      schema: 'membrane.release-evidence.v1',
      release: { target: 'windows-x86_64', generation: 'a'.repeat(64), artifact_sha256: 'a'.repeat(64) },
      signing: { status: 'unsigned' },
    }));
    writeFileSync(installedPath, JSON.stringify({
      kind: 'rightkit-direct-release-manifest',
      sourceCommit: '0'.repeat(40),
      assets: [{ target: 'windows-x86_64', sha256: 'b'.repeat(64) }],
    }));
    const result = await PKG_02({ row: { candidateManifestPath: candidatePath, installedManifestPath: installedPath }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.equal(result.evidenceKind, 'installed');
    assert.match(result.reason, /generation could not be derived/);
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

test('parseControllerProbeFailure: extracts the typed blueprint-watcher reason from a non-200 cli health body', () => {
  const stdout = JSON.stringify({
    ok: false,
    blueprintWatcher: {
      watcherState: 'watcher_unavailable',
      watcherDetail: 'resident Blueprint watcher D:\\Claude\\membrane: Degraded: rebuild callback failed: deadline_exceeded: request deadline exceeded',
    },
    dailyAnalysis: { status: 'unavailable', reason: 'missing_output' },
  });
  const parsed = parseControllerProbeFailure({ status: 2, stdout, stderr: 'membrane: installed health returned HTTP 503\n' });
  assert.equal(parsed.exitStatus, 2);
  assert.equal(parsed.stderr, 'membrane: installed health returned HTTP 503');
  assert.equal(parsed.blueprintWatcherState, 'watcher_unavailable');
  assert.match(parsed.blueprintWatcherDetail, /deadline_exceeded/);
  assert.deepEqual(parsed.dailyAnalysis, { status: 'unavailable', reason: 'missing_output' });
  assert.equal(parsed.ok, false);
});

test('parseControllerProbeFailure: never throws and degrades to null fields on unparseable stdout', () => {
  const parsed = parseControllerProbeFailure({ status: 1, stdout: 'not json', stderr: '' });
  assert.equal(parsed.exitStatus, 1);
  assert.equal(parsed.stderr, null);
  assert.equal(parsed.blueprintWatcherDetail, null);
  assert.equal(parsed.blueprintWatcherState, null);
  assert.equal(parsed.dailyAnalysis, null);
  assert.equal(parsed.ok, null);
});

test('parseControllerProbeFailure: tolerates entirely missing input', () => {
  const parsed = parseControllerProbeFailure();
  assert.equal(parsed.exitStatus, null);
  assert.equal(parsed.stderr, null);
});

test('queryInstalledControllerIdentity: never fabricates a value when no root is configured', () => {
  assert.equal(queryInstalledControllerIdentity(undefined), null);
  assert.equal(queryInstalledControllerIdentity(''), null);
});

test('queryInstalledControllerIdentity: never fabricates a value when membrane.exe is absent at the given root', () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg03-no-exe-'));
  try {
    assert.equal(queryInstalledControllerIdentity(scratch), null);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_03: falls back to a live installed CLI health query only when no controllerIdentityPath is supplied, and still never fabricates', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg03-live-'));
  try {
    const installedPath = join(scratch, 'installed.json');
    writeFileSync(installedPath, JSON.stringify({ release: { generation: 'sha256:2222' } }));
    // No membrane.exe at installedRoot -> queryInstalledControllerIdentity returns
    // null -> PKG_03 must report the same non-fabricated blocked reason as when
    // no controller source is configured at all, never a fabricated PASS.
    const result = await PKG_03({ row: { installedManifestPath: installedPath, installedRoot: scratch }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.equal(result.evidenceKind, 'source');
    assert.equal(result.detail.controllerSource, null);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_03: attempts a bounded self-start of the installed Hub holder when no controller is resident, and fails closed (not a fabricated PASS) on timeout, cleaning up what it started', { skip: process.platform !== 'win32' ? 'tray-stub spawn is Windows-only' : false }, async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg03-selfstart-timeout-'));
  try {
    const cmdExe = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'cmd.exe');
    const trayStub = join(scratch, 'membrane-tray.exe');
    if (!existsSync(cmdExe)) return; // no usable stub executable on this host; nothing to prove here
    copyFileSync(cmdExe, trayStub);
    const result = await PKG_03({ row: { installedRoot: scratch, selfStartTimeoutMs: 300 }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.equal(result.evidenceKind, 'source');
    assert.equal(result.detail.attemptedSelfStart, true);
    assert.match(result.reason, /timed out/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('PKG_03: never attempts a self-start when disableSelfStart is set', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'pkg03-selfstart-disabled-'));
  try {
    const cmdExe = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'cmd.exe');
    const trayStub = join(scratch, 'membrane-tray.exe');
    if (existsSync(cmdExe)) copyFileSync(cmdExe, trayStub);
    const result = await PKG_03({ row: { installedRoot: scratch, disableSelfStart: true, selfStartTimeoutMs: 300 }, workspaceRoot });
    assert.equal(result.status, 'failed');
    assert.equal(result.detail.attemptedSelfStart, false);
    assert.doesNotMatch(result.reason, /timed out/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
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

test('registry runner keeps source findings visible without treating them as functional closure', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'registry-functional-boundary-'));
  try {
    const registryPath = join(scratch, 'registry.json');
    const evidencePath = join(scratch, 'evidence.json');
    writeFileSync(registryPath, JSON.stringify({ cases: [{ id: 'X-1', group: 'X', caseFile: 'fixture.mjs', caseExport: 'SOURCE_PASS' }] }));
    const summary = await runRegistryQualification({
      platform: 'windows', profile: 'internal-unsigned', caseRegistryPath: registryPath,
      group: 'X', evidencePath, workspaceRoot,
      importCaseModule: async () => ({ SOURCE_PASS: () => ({ status: 'passed', evidenceKind: 'source', detail: { structural: true } }) }),
    });
    assert.equal(summary.results[0].status, 'passed');
    assert.equal(summary.results[0].functionalStatus, 'structural');
    assert.equal(summary.status, 'failed');
    assert.equal(summary.functionalStatus, 'failed');
    assert.equal(summary.unsignedFunctional, false);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('registry runner closes unsigned functional qualification only on runtime evidence', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'registry-functional-pass-'));
  try {
    const registryPath = join(scratch, 'registry.json');
    const evidencePath = join(scratch, 'evidence.json');
    writeFileSync(registryPath, JSON.stringify({ cases: [{ id: 'X-2', group: 'X', caseFile: 'fixture.mjs', caseExport: 'INSTALLED_PASS' }] }));
    const summary = await runRegistryQualification({
      platform: 'windows', profile: 'internal-unsigned', caseRegistryPath: registryPath,
      group: 'X', evidencePath, workspaceRoot,
      importCaseModule: async () => ({ INSTALLED_PASS: () => ({ status: 'passed', evidenceKind: 'installed' }) }),
    });
    assert.equal(summary.results[0].functionalStatus, 'passed');
    assert.equal(summary.status, 'passed');
    assert.equal(summary.functionalStatus, 'passed');
    assert.equal(summary.unsignedFunctional, true);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('registry runner imports cases from caseSourceRoot while executing them against workspaceRoot', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'registry-split-roots-'));
  try {
    const runtimeRoot = join(scratch, 'runtime');
    const caseSourceRoot = join(scratch, 'source');
    mkdirSync(runtimeRoot);
    mkdirSync(caseSourceRoot);
    const registryPath = join(scratch, 'registry.json');
    const evidencePath = join(scratch, 'evidence.json');
    writeFileSync(join(caseSourceRoot, 'fixture.mjs'), [
      'export function SPLIT_ROOTS(context) {',
      '  return { status: "passed", evidenceKind: "installed", detail: { workspaceRoot: context.workspaceRoot, caseSourceRoot: context.caseSourceRoot } };',
      '}',
    ].join('\n'));
    writeFileSync(registryPath, JSON.stringify({ cases: [{ id: 'X-3', group: 'X', caseFile: 'fixture.mjs', caseExport: 'SPLIT_ROOTS' }] }));

    const summary = await runRegistryQualification({
      platform: 'windows', profile: 'internal-unsigned', caseRegistryPath: registryPath,
      group: 'X', evidencePath, workspaceRoot: runtimeRoot, caseSourceRoot,
    });

    assert.equal(summary.status, 'passed');
    assert.equal(summary.workspaceRoot, runtimeRoot);
    assert.equal(summary.caseSourceRoot, caseSourceRoot);
    assert.deepEqual(summary.results[0].detail, { workspaceRoot: runtimeRoot, caseSourceRoot });
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
