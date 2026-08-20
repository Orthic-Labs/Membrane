import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runWholeTaskBenchmark, writeHostReceipt, DEFAULT_CORPUS_PATH } from './runner.mjs';
import { commitSalt } from './holdout.mjs';

const salt = 'mbr804-runner-test-salt-v1';
const holdoutCommitment = commitSalt(salt);
const passingExecute = async () => ({ success: true, latencyMs: 100, tokens: 50, cacheHit: true, costUsd: 0.001 });

test('degrades explicitly when no executor is configured (no fabrication)', async () => {
  const result = await runWholeTaskBenchmark({ salt, holdoutCommitment });
  assert.equal(result.status, 'degraded');
  assert.equal(result.reason, 'executor-not-configured');
});

test('degrades explicitly when the corpus path does not exist', async () => {
  const result = await runWholeTaskBenchmark({ corpusPath: '/nonexistent/mbr804-corpus.json', salt, holdoutCommitment, execute: passingExecute });
  assert.equal(result.status, 'degraded');
  assert.equal(result.reason, 'corpus-not-found');
});

test('the default corpus is the labelled fixture and can never produce status complete', async () => {
  const result = await runWholeTaskBenchmark({ corpusPath: DEFAULT_CORPUS_PATH, salt, holdoutCommitment, execute: passingExecute });
  assert.equal(result.status, 'source-ready');
  assert.equal(result.receipt.status, 'source-ready');
  assert.equal(result.receipt.task.corpus.fixture, true);
});

test('rejects a run whose salt does not match its pre-committed holdout commitment', async () => {
  const result = await runWholeTaskBenchmark({ corpusPath: DEFAULT_CORPUS_PATH, salt: 'a-different-unc0mmitted-salt', holdoutCommitment, execute: passingExecute });
  assert.equal(result.status, 'invalid');
  assert.match(result.reason, /does not match the pre-committed commitment/);
});

test('degrades explicitly when host receipts are not configured for a non-fixture corpus', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'mbr804-corpus-'));
  try {
    const corpusPath = join(dir, 'corpus.json');
    writeFileSync(corpusPath, JSON.stringify({ id: 'real-corpus-not-a-fixture', version: '1', cases: [{ id: 'x1', category: 'task_success' }, { id: 'x2', category: 'task_success' }] }));
    const result = await runWholeTaskBenchmark({ corpusPath, salt, holdoutCommitment, execute: passingExecute });
    assert.equal(result.status, 'degraded');
    assert.equal(result.reason, 'host-receipts-not-configured');
    assert.deepEqual(result.missing, ['macos']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('writeHostReceipt records real host identity and a caller-asserted measured flag, never a fabricated pass', () => {
  const dir = mkdtempSync(join(tmpdir(), 'mbr804-hostreceipt-'));
  try {
    const { path, receipt } = writeHostReceipt({ evidenceRoot: dir, platform: 'macos', releaseGeneration: 'release-test-1' });
    assert.ok(path.includes('macos') && path.endsWith('receipt.json'));
    assert.equal(receipt.status, 'measured');
    assert.equal(receipt.release, 'release-test-1');
    assert.match(receipt.hardware, /^macos-/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('end to end: a real (non-fixture) corpus, a real un-rerun bakeoff, and a real Mac host receipt produce a receipt that the existing strict verifier reports as passed', async () => {
  const corpusDir = mkdtempSync(join(tmpdir(), 'mbr804-e2e-corpus-'));
  const evidenceDir = mkdtempSync(join(tmpdir(), 'mbr804-e2e-evidence-'));
  try {
    const corpusPath = join(corpusDir, 'corpus.json');
    writeFileSync(corpusPath, JSON.stringify({
      id: 'mbr804-e2e-synthetic-corpus', version: '1',
      cases: Array.from({ length: 10 }, (_, i) => ({ id: `case-${i}`, category: 'task_success' })),
    }));
    const releaseGeneration = 'release-e2e-1';
    const macos = writeHostReceipt({ evidenceRoot: evidenceDir, platform: 'macos', releaseGeneration });

    const result = await runWholeTaskBenchmark({
      corpusPath,
      salt,
      holdoutCommitment,
      release: { commit: 'b'.repeat(40), generation: releaseGeneration, client: 'client-e2e', service: 'service-e2e' },
      models: ['model-e2e'],
      hardware: { macos: 'Apple-e2e' },
      hostReceiptPaths: { macos: macos.path },
      execute: passingExecute,
    });

    assert.equal(result.status, 'ran');
    assert.equal(result.receipt.status, 'complete');
    assert.equal(result.receipt.task.corpus.fixture, false);
    assert.equal(result.receipt.bakeoff.immutable, true);
    assert.ok(result.receipt.bakeoff.arms.length >= 2);
    assert.equal(result.receipt.hosts.macos.status, 'measured');
    assert.equal(result.receipt.hosts.macos.release, releaseGeneration);
    assert.equal(result.receipt.metrics.task_success.measured, true);
    assert.equal(result.verification.status, 'passed');
  } finally {
    rmSync(corpusDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});

test('a failing case is recorded in failures and never silently dropped', async () => {
  const corpusDir = mkdtempSync(join(tmpdir(), 'mbr804-fail-corpus-'));
  const evidenceDir = mkdtempSync(join(tmpdir(), 'mbr804-fail-evidence-'));
  try {
    const corpusPath = join(corpusDir, 'corpus.json');
    writeFileSync(corpusPath, JSON.stringify({ id: 'mbr804-fail-corpus', version: '1', cases: [{ id: 'ok-1', category: 'task_success' }, { id: 'bad-1', category: 'unauthorized_context' }] }));
    const releaseGeneration = 'release-fail-1';
    const macos = writeHostReceipt({ evidenceRoot: evidenceDir, platform: 'macos', releaseGeneration });

    const result = await runWholeTaskBenchmark({
      corpusPath, salt, holdoutCommitment,
      release: { commit: 'c'.repeat(40), generation: releaseGeneration, client: 'client', service: 'service' },
      models: ['model'], hardware: { macos: 'Apple' },
      hostReceiptPaths: { macos: macos.path },
      execute: async ({ case: c }) => (c.id === 'bad-1' ? { success: false, contextError: { unauthorized: true } } : { success: true, latencyMs: 10, tokens: 5, cacheHit: false, costUsd: 0 }),
    });

    assert.equal(result.receipt.failures.length, 1);
    assert.equal(result.receipt.failures[0].id, 'bad-1');
    assert.ok(result.receipt.metrics.context_errors.value >= 1);
  } finally {
    rmSync(corpusDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});
