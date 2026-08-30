import test from 'node:test';
import assert from 'node:assert/strict';
import { parseArgs, runCli } from './run-whole-task-benchmark.mjs';
import { commitSalt } from '../../../tests/benchmarks/whole-task/holdout.mjs';

test('parseArgs reads every documented flag', () => {
  const args = parseArgs([
    '--corpus', '/tmp/corpus.json', '--salt', 's3cr3t-value', '--holdout-commitment', 'a'.repeat(64),
    '--holdout-fraction', '0.2', '--release-commit', 'b'.repeat(40), '--release-generation', 'g1',
    '--release-client', 'claude_code', '--release-service', 'membrane-hub', '--model', 'model-a', '--model', 'model-b',
    '--hardware-macos', 'Apple', '--host-receipt-macos', '/tmp/mac.json',
    '--publication-disposition', 'internal-only', '--publication-approved', 'true',
  ]);
  assert.equal(args.corpusPath, '/tmp/corpus.json');
  assert.equal(args.salt, 's3cr3t-value');
  assert.equal(args.holdoutFraction, 0.2);
  assert.deepEqual(args.models, ['model-a', 'model-b']);
  assert.equal(args.publicationApproved, true);
});

test('runCli with no --salt/--holdout-commitment degrades explicitly rather than guessing a split', async () => {
  const result = await runCli([], { execute: async () => ({ success: true, latencyMs: 1, tokens: 1, cacheHit: false, costUsd: 0 }) });
  assert.equal(result.status, 'degraded');
  assert.equal(result.reason, 'holdout-not-configured');
});

test('runCli fails closed when a pinned bakeoff input has drifted', async () => {
  const salt = 'cli-test-salt-value-1';
  const result = await runCli(['--salt', salt, '--holdout-commitment', commitSalt(salt)], {
    execute: async () => ({ success: true, latencyMs: 1, tokens: 1, cacheHit: false, costUsd: 0 }),
  });
  assert.equal(result.status, 'degraded');
  assert.equal(result.reason, 'bakeoff-reference-unavailable');
});
