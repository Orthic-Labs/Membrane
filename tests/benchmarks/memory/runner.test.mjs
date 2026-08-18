import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runMemoryBenchmark, runAllMemoryBenchmarks } from './runner.mjs';

const validRaw = {
  identity: { corpus: { id: 'synthetic', version: 'v1' }, model: 'fixture-model', hardware: 'cpu-fixture', release: 'release-fixture' },
  metrics: { retrieval: { recall: 1 }, admission: { accepted: 1 }, product: { latencyMs: 1 } },
  raw: { cases: [], fixture: true, note: 'synthetic fixture, not a measured benchmark run' }
};

test('degrades explicitly when no dataset root is configured (no network, no fabrication)', async () => {
  const result = await runMemoryBenchmark({ benchmark: 'LoCoMo' });
  assert.equal(result.status, 'degraded');
  assert.equal(result.reason, 'dataset-root-not-configured');
  assert.equal(result.componentUnderTest, 'crypt-memory');
  assert.equal('metrics' in result, false);
});

test('degrades explicitly when the dataset root does not exist on disk (never downloads it)', async () => {
  const result = await runMemoryBenchmark({ benchmark: 'LongMemEval', datasetRoot: '/nonexistent/does-not-exist-mbr-805' });
  assert.equal(result.status, 'degraded');
  assert.equal(result.reason, 'dataset-not-found');
});

test('degrades explicitly when a dataset root exists but no executor is supplied', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'mbr-805-'));
  try {
    const result = await runMemoryBenchmark({ benchmark: 'BEAM', datasetRoot: dir });
    assert.equal(result.status, 'degraded');
    assert.equal(result.reason, 'executor-not-configured');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('runs and reports a verified result when dataset root and executor are both present', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'mbr-805-'));
  try {
    const result = await runMemoryBenchmark({ benchmark: 'LoCoMo', datasetRoot: dir, execute: async () => validRaw });
    assert.equal(result.status, 'ran');
    assert.equal(result.componentUnderTest, 'crypt-memory');
    assert.equal(result.benchmark, 'LoCoMo');
    assert.deepEqual(result.metrics.retrieval, { recall: 1 });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('reports invalid, not a fabricated pass, when the executor conflates a Membrane/managed score', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'mbr-805-'));
  try {
    const result = await runMemoryBenchmark({ benchmark: 'LoCoMo', datasetRoot: dir, execute: async () => ({ ...validRaw, membraneScore: 99 }) });
    assert.equal(result.status, 'invalid');
    assert.match(result.reason, /Membrane or managed score attribution is forbidden/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('reports invalid, not a fabricated pass, when the executor omits required identity', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'mbr-805-'));
  try {
    const result = await runMemoryBenchmark({ benchmark: 'LoCoMo', datasetRoot: dir, execute: async () => ({ ...validRaw, identity: { ...validRaw.identity, hardware: '' } }) });
    assert.equal(result.status, 'invalid');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('runAllMemoryBenchmarks covers every declared benchmark and one degrade never blocks another', async () => {
  const report = await runAllMemoryBenchmarks({});
  assert.equal(report.results.length, 3);
  assert.deepEqual(report.results.map(r => r.benchmark).sort(), ['BEAM', 'LoCoMo', 'LongMemEval']);
  assert.ok(report.results.every(r => r.status === 'degraded'));
});

test('rejects an unsupported benchmark name', async () => {
  await assert.rejects(() => runMemoryBenchmark({ benchmark: 'NotARealBenchmark' }), /unsupported benchmark/);
});
