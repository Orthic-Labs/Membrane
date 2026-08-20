import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { verifyWholeTaskBenchmark } from '../../scripts/qualification/verify-whole-task-benchmark.mjs';

const hash = 'a'.repeat(64);
const root = mkdtempSync(join(tmpdir(), 'mbr804-'));
const artifact = name => { const path = join(root, name); writeFileSync(path, name); return { path, hash: createHash('sha256').update(name).digest('hex') }; };
const config = artifact('config.json'); const input = artifact('input.json'); const receipt = artifact('bakeoff.json'); const mac = artifact('mac.json'); const win = artifact('win.json');
const base = { schema: 'membrane.mbr804-whole-task.v1', status: 'complete', run: { run_id: 'r1', started_at: '2026-01-01T00:00:00Z', ended_at: '2026-01-01T00:01:00Z', measured: true }, task: { task_id: 'MBR-804', release: { commit: 'b'.repeat(40), generation: 'release-1', client: 'client-1', service: 'service-1' }, corpus: { id: 'c', version: '1', sha256: hash, cases: 2 }, models: ['model-1'], hardware: { macos: 'Apple', windows: 'x64' }, controls: { unauthorized_context: true, stale_context: true, missed_authoritative: true, cache_policy: 'bounded' } }, bakeoff: { config: config.path, config_sha256: config.hash, input: input.path, input_sha256: input.hash, receipt: receipt.path, receipt_sha256: receipt.hash, immutable: true, arms: ['baseline', 'membrane'] }, hosts: { macos: { receipt: mac.path, receipt_sha256: mac.hash, release: 'release-1', hardware: 'Apple', status: 'measured' }, windows: { receipt: win.path, receipt_sha256: win.hash, release: 'release-1', hardware: 'x64', status: 'measured' } }, metrics: Object.fromEntries(['task_success', 'context_errors', 'latency_ms', 'tokens', 'cache', 'cost'].map(k => [k, { measured: true, value: 1, unit: 'count' }])), failures: [], publication: { disposition: 'not-published', approved: false } };

test('synthetic complete receipt passes', () => assert.equal(verifyWholeTaskBenchmark(base).status, 'passed'));
test('source-ready never passes', () => assert.equal(verifyWholeTaskBenchmark({ ...base, status: 'source-ready' }).status, 'open'));
test('stale host fails closed', () => assert.match(verifyWholeTaskBenchmark({ ...base, hosts: { ...base.hosts, windows: { ...base.hosts.windows, status: 'stale' } } }).reason, /windows/));
test('unmeasured metric fails closed', () => assert.match(verifyWholeTaskBenchmark({ ...base, metrics: { ...base.metrics, cost: { measured: false } } }).reason, /measured/));
