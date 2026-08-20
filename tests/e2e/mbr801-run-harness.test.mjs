import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { SCENARIOS, resolvePlatform, runInstalledPathHarness } from '../../scripts/qualification/run.mjs';
import { verifyMbr801Evidence } from '../../scripts/qualification/verify-mbr801-evidence.mjs';

const COMMIT = 'b'.repeat(40);
const RELEASE_GENERATION = 'release-mbr801-test';
const ARTIFACT_SHA256 = 'c'.repeat(64);

function fakeIdentity() {
  return async () => ({ client: 'claude-code', model: 'test-model-1', host: 'test-host' });
}
function fakeSignedBuild() {
  return async () => ({ installed_execution: true, signature_status: 'passed', artifact_sha256: ARTIFACT_SHA256, release_generation: RELEASE_GENERATION });
}
function fakeScenarioRunner(platform, { failScenario, duplicateOf } = {}) {
  return async ({ scenario }) => {
    if (scenario === failScenario) return { trace_id: null, gates: {} };
    const traceId = scenario === duplicateOf ? `${platform}-trace-${SCENARIOS[0]}` : `${platform}-trace-${scenario}`;
    return { trace_id: traceId, gates: { provider: 'passed', delivery: 'passed', outcome: 'passed' } };
  };
}
function fakeBenchmark() {
  return async () => ({ status: 'complete' });
}

async function runPlatform(evidenceRoot, platform, overrides = {}) {
  return runInstalledPathHarness({
    task: 'MBR-801',
    workspaceRoot: process.cwd(),
    evidenceRoot,
    platform,
    gitCommit: () => COMMIT,
    resolveHostIdentity: fakeIdentity(),
    verifySignedBuild: fakeSignedBuild(),
    scenarioRunner: fakeScenarioRunner(platform),
    benchmarkRunner: fakeBenchmark(),
    now: () => '2026-08-08T00:00:00.000Z',
    ...overrides,
  });
}

test('resolvePlatform: only explicit macOS is accepted', () => {
  assert.equal(resolvePlatform('macos'), 'macos');
  assert.equal(resolvePlatform(undefined), 'macos');
  assert.throws(() => resolvePlatform('windows'), /Mac-only/);
  assert.throws(() => resolvePlatform('bogus'), /Mac-only/);
});

test('runInstalledPathHarness: unsupported task returns open without touching disk', async () => {
  const result = await runInstalledPathHarness({ task: 'MBR-999' });
  assert.equal(result.status, 'open');
});

test('runInstalledPathHarness: macOS run completes all ten scenarios with unique traces and a passed receipt', async () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), 'mbr801-harness-'));
  const result = await runPlatform(evidenceRoot, 'macos');
  assert.equal(result.status, 'passed');
  assert.equal(result.receipt.scenario_count, 10);
  assert.equal(result.receipt.scenarios_passed, 10);
  assert.equal(result.receipt.traces.length, 10);
  const traceIds = result.receipt.traces.map((trace) => trace.trace_id);
  assert.equal(new Set(traceIds).size, 10, 'all ten trace IDs must be unique');
  for (const trace of result.receipt.traces) {
    assert.equal(trace.gates.provider, 'passed');
    assert.equal(trace.gates.delivery, 'passed');
    assert.equal(trace.gates.outcome, 'passed');
  }
  assert.deepEqual(result.receipt.reasons, []);
});

test('acceptance: the Mac receipt verifies as passed', async () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), 'mbr801-harness-'));
  const macos = await runPlatform(evidenceRoot, 'macos');
  assert.equal(macos.status, 'passed');

  const macTraceIds = macos.receipt.traces.map((trace) => trace.trace_id);
  assert.equal(new Set(macTraceIds).size, 10, 'all ten Mac trace IDs must be unique');

  const verification = verifyMbr801Evidence({
    macos: macos.receiptPath, commit: COMMIT, releaseGeneration: RELEASE_GENERATION,
  });
  assert.equal(verification.status, 'passed');
  assert.equal(verification.platforms.macos.status, 'passed');
});

test('a scenario that fails to produce a trace fails the receipt closed', async () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), 'mbr801-harness-'));
  const result = await runPlatform(evidenceRoot, 'macos', { scenarioRunner: fakeScenarioRunner('macos', { failScenario: SCENARIOS[3] }) });
  assert.equal(result.status, 'incomplete');
  assert.ok(result.receipt.reasons.some((reason) => reason.includes('trace id')));
});

test('duplicate trace IDs across scenarios fail the receipt closed', async () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), 'mbr801-harness-'));
  const result = await runPlatform(evidenceRoot, 'macos', { scenarioRunner: fakeScenarioRunner('macos', { duplicateOf: SCENARIOS[1] }) });
  assert.equal(result.status, 'incomplete');
  assert.ok(result.receipt.reasons.some((reason) => reason.includes('not unique')));
});

test('an unsigned or unverifiable build fails the receipt closed even if all scenarios pass', async () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), 'mbr801-harness-'));
  const result = await runPlatform(evidenceRoot, 'macos', {
    verifySignedBuild: async () => ({ installed_execution: false, signature_status: 'unavailable', artifact_sha256: null, release_generation: null, reason: 'no manifest supplied' }),
  });
  assert.equal(result.status, 'incomplete');
  assert.ok(result.receipt.reasons.some((reason) => reason.includes('installed execution')));
  assert.ok(result.receipt.reasons.some((reason) => reason.includes('signature status')));
});

test('an incomplete benchmark fails the receipt closed even if all scenarios pass', async () => {
  const evidenceRoot = mkdtempSync(join(tmpdir(), 'mbr801-harness-'));
  const result = await runPlatform(evidenceRoot, 'macos', { benchmarkRunner: async () => ({ status: 'blocked_incomplete_path' }) });
  assert.equal(result.status, 'incomplete');
  assert.ok(result.receipt.reasons.some((reason) => reason.includes('benchmark')));
});
