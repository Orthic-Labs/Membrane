import test from 'node:test';
import assert from 'node:assert/strict';
import { parseArgs, runReport } from './run-memory-benchmarks.mjs';

test('parseArgs reads --benchmark and --dataset-root', () => {
  assert.deepEqual(parseArgs(['--benchmark', 'LoCoMo', '--dataset-root', '/tmp/x']), { benchmark: 'LoCoMo', datasetRoot: '/tmp/x' });
  assert.deepEqual(parseArgs([]), {});
});

test('runReport with no flags degrades every benchmark explicitly (no dataset configured, nothing downloaded)', async () => {
  const report = await runReport([]);
  assert.equal(report.schema, 'membrane.memory-benchmark-run-report.v1');
  assert.equal(report.results.length, 3);
  assert.ok(report.results.every(r => r.status === 'degraded' && r.reason === 'dataset-root-not-configured'));
});

test('runReport with --benchmark scopes to a single benchmark', async () => {
  const report = await runReport(['--benchmark', 'BEAM']);
  assert.equal(report.results.length, 1);
  assert.equal(report.results[0].benchmark, 'BEAM');
});

test('runReport with an unconfigured dataset root reports dataset-not-found, never a fabricated result', async () => {
  const report = await runReport(['--benchmark', 'LongMemEval', '--dataset-root', '/nonexistent/does-not-exist-mbr-805']);
  assert.equal(report.results[0].status, 'degraded');
  assert.equal(report.results[0].reason, 'dataset-not-found');
});

test('runReport rejects an unsupported benchmark name', async () => {
  await assert.rejects(() => runReport(['--benchmark', 'NotReal']), /unsupported benchmark/);
});
