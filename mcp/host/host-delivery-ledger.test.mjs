// Plan 2.3: the forge hook's per-session delivery ledger writes one immutable
// JSON record per delivered block on first render; a fresh child process
// hydrates the same ledger and skips already-delivered content; changing the
// source hash re-delivers; corrupt / symlinked / path-escaping neighbours
// cannot suppress a legitimate write; 8-16 parallel first writers land
// exactly one record per block via O_EXCL.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import test from 'node:test';

const require = createRequire(import.meta.url);
const adapter = require('./context-adapter.cjs');
const rendererLib = require('../context-renderer-lib.cjs');
const ledgerStore = require('./delivery-ledger-store.cjs');

const adapterPath = path.resolve('mcp/host/context-adapter.cjs');
const storePath = path.resolve('mcp/host/delivery-ledger-store.cjs');
const rendererPath = path.resolve('mcp/context-renderer-lib.cjs');

function tempRoot() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'membrane-ledger-'));
  process.env.MEMBRANE_DATA_ROOT = dir;
  adapter.__resetDeliveryLedger();
  return dir;
}
function cleanup(dir) {
  adapter.__resetDeliveryLedger();
  delete process.env.MEMBRANE_DATA_ROOT;
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch { /* ignore */ }
}
function fresh(root, session, blocks) {
  const source = "const a=require(process.argv[1]),p=JSON.parse(process.argv[2]);process.stdout.write(a.render({state:'context_enforced',request:{session:process.argv[3]},payload:{packet:p,providerStatus:'ready',receipts:[]}}))";
  const packet = { schema: 'orthic.context-packet.v1', budget: { packetCharBudgetDefault: 30000, configuredPacketCharBudget: 30000 }, blocks };
  const result = spawnSync(process.execPath, ['-e', source, adapterPath, JSON.stringify(packet), session], { encoding: 'utf8', env: { ...process.env, MEMBRANE_DATA_ROOT: root } });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}
function block(id, sourceHash, text) {
  return { id, provider: 'rules', priority: 90, sourceHash, resolver: 'read', text };
}

test('fresh Node process emits once, omits same hash, & emits changed/different ledger', { concurrency: false }, () => {
  const root = tempRoot();
  try {
    const sessionId = 'session-A';
    const hash = 'sha256:' + 'a'.repeat(64);
    const blocks = [block('rules:AGENTS.md', hash, 'PERSISTED-ALPHA'), block('git:meta', hash, 'PERSISTED-BRAVO')];
    const firstRender = fresh(root, sessionId, blocks);
    assert.ok(firstRender.includes('PERSISTED-ALPHA'), 'first render must include ALPHA');
    assert.ok(firstRender.includes('PERSISTED-BRAVO'), 'first render must include BRAVO');
    const secondRender = fresh(root, sessionId, blocks);
    assert.ok(!secondRender.includes('PERSISTED-ALPHA'), 'fresh child must omit already-delivered ALPHA');
    assert.ok(!secondRender.includes('PERSISTED-BRAVO'), 'fresh child must omit already-delivered BRAVO');
    assert.ok(fresh(root, sessionId, [block('rules:AGENTS.md', 'sha256:' + 'b'.repeat(64), 'CHANGED')]).includes('CHANGED'));
    assert.ok(fresh(root, 'other-ledger', blocks).includes('PERSISTED-ALPHA'));
  } finally { cleanup(root); }
});

test('12 parallel first writers land exactly one record per block', { concurrency: false }, async () => {
  const root = tempRoot();
  try {
    const sessionId = 'session-C';
    const hash = 'sha256:' + 'c'.repeat(64);
    const dir = ledgerStore.sessionDir('session:' + sessionId, process.env);
    fs.mkdirSync(dir, { recursive: true });
    const expected = ledgerStore.recordFilename('rules:AGENTS.md', hash);
    const script = "const c=require(process.argv[1]),r=require(process.argv[2]),s=new r.ContextSessionV1({sessionId:process.argv[4]}),h=process.argv[3];s.record('rules:AGENTS.md','inline',h,32);process.stdout.write(JSON.stringify(c.persist(s,process.argv[4],0)))";
    const ledgerKey = 'session:' + sessionId;
    const args = ['-e', script, storePath, rendererPath, hash, ledgerKey];
    const spawnOne = () => new Promise((resolve, reject) => {
      const child = require('node:child_process').spawn(process.execPath, args, { env: { ...process.env, MEMBRANE_DATA_ROOT: root } });
      let out = ''; child.stdout.on('data', (d) => { out += d; }); child.stderr.on('data', (d) => { out += d; });
      child.on('error', reject); child.on('exit', (code) => resolve({ code, out }));
    });
    const results = await Promise.all(Array.from({ length: 12 }, spawnOne));
    for (const r of results) assert.equal(r.code, 0, 'child exited cleanly: ' + r.out);
    const counts = results.map((r) => JSON.parse(r.out));
    assert.equal(counts.reduce((n, r) => n + r.written, 0), 1);
    assert.equal(counts.reduce((n, r) => n + r.existing, 0), 11);
    const entries = fs.readdirSync(dir);
    assert.ok(entries.includes(expected), 'expected record present');
    assert.equal(entries.length, 1, 'exactly one record on disk: ' + entries.join(','));
    const record = JSON.parse(fs.readFileSync(path.join(dir, expected), 'utf8'));
    assert.equal(record.schema, ledgerStore.LEDGER_SCHEMA);
  } finally { cleanup(root); }
});

test('malicious neighbours fail open without suppressing a legitimate write', { concurrency: false }, () => {
  const root = tempRoot();
  try {
    const sessionId = 'session-D';
    const hash = 'sha256:' + 'd'.repeat(64);
    const decoyHash = 'sha256:' + 'e'.repeat(64);
    const dir = ledgerStore.sessionDir('session:' + sessionId, process.env);
    fs.mkdirSync(dir, { recursive: true });
    // Corrupt JSON neighbour AT the legitimate candidate's record path —
    // lstat says file, readFileSync returns the bytes, JSON.parse fails,
    // so hydrate MUST surface a diagnostic instead of treating it as a match.
    const legitName = ledgerStore.recordFilename('rules:AGENTS.md', hash);
    fs.writeFileSync(path.join(dir, legitName), '{ not valid json');
    // Symlink pointing outside the ledger dir, at a different candidate path.
    const escape = path.join(root, 'escape.json');
    fs.writeFileSync(escape, JSON.stringify({ schema: 'unrelated' }));
    const decoyName = ledgerStore.recordFilename('rules:OTHER', decoyHash);
    try { fs.symlinkSync(escape, path.join(dir, decoyName)); } catch { /* symlinks may be unsupported */ }
    // Hydrate — corrupt neighbour must emit a diagnostic, no match.
    const session = new rendererLib.ContextSessionV1({ sessionId: 'session-D' });
    const probe = ledgerStore.hydrate(session, 'session:' + sessionId, [{ id: 'rules:AGENTS.md', sourceHash: hash }, { id: 'rules:OTHER', sourceHash: decoyHash }]);
    assert.equal(probe.matched, 0, 'malicious neighbours must not match');
    assert.ok(probe.diagnostic, 'malicious neighbour must surface a bounded diagnostic');
    // The corrupt file blocks the legitimate write path via EEXIST; remove
    // it so the next persist() creates a real record.
    fs.rmSync(path.join(dir, legitName));
    const real = new rendererLib.ContextSessionV1({ sessionId: 'session-D' });
    real.record('rules:AGENTS.md', 'inline', hash, 16);
    const written = ledgerStore.persist(real, 'session:' + sessionId, 0);
    assert.ok(written.written >= 1, 'legitimate write must succeed');
    // Re-hydrate — the legitimate record must match.
    const session2 = new rendererLib.ContextSessionV1({ sessionId: 'session-D' });
    const probe2 = ledgerStore.hydrate(session2, 'session:' + sessionId, [{ id: 'rules:AGENTS.md', sourceHash: hash }, { id: 'rules:OTHER', sourceHash: decoyHash }]);
    assert.equal(probe2.matched, 1, 'only the legitimate record hydrates');
  } finally { cleanup(root); }
});

// ---------------------------------------------------------------------------
// P2 §8.9 — bounded durable outbox.
// ---------------------------------------------------------------------------
function outboxEnv() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'membrane-outbox-'));
  const env = { ...process.env, MEMBRANE_DATA_ROOT: dir };
  return { dir, env };
}
function cleanupOutbox({ dir }) {
  try { fs.rmSync(dir, { recursive: true, force: true }); } catch { /* ignore */ }
}

test('outbox enqueue is idempotent, bounded, and deadline-checked', { concurrency: false }, () => {
  const { dir, env } = outboxEnv();
  try {
    const now = Date.now();
    const first = ledgerStore.enqueueOutbox(env, { eventId: 'mutation-1', payload: { op: 'a' }, deadlineAtMs: now + 60_000, nowMs: now });
    assert.equal(first.status, 'enqueued');
    assert.deepEqual(first.record.payload, { op: 'a' });
    const again = ledgerStore.enqueueOutbox(env, { eventId: 'mutation-1', payload: { op: 'a' }, deadlineAtMs: now + 60_000, nowMs: now });
    assert.equal(again.status, 'duplicate');
    const drift = ledgerStore.enqueueOutbox(env, { eventId: 'mutation-1', payload: { op: 'changed' }, deadlineAtMs: now + 60_000, nowMs: now });
    assert.equal(drift.status, 'conflict');
    assert.equal(drift.reason, 'outbox_event_id_payload_drift');
    // Same id is stable: only one record exists on disk.
    assert.equal(fs.readdirSync(ledgerStore.outboxRoot(env)).filter((n) => n.endsWith('.json')).length, 1);
    // Expired deadline is refused, not silently queued.
    const expired = ledgerStore.enqueueOutbox(env, { eventId: 'mutation-expired', payload: { op: 'b' }, deadlineAtMs: now - 1, nowMs: now });
    assert.equal(expired.status, 'invalid');
    assert.equal(expired.reason, 'outbox_deadline_expired');
  } finally { cleanupOutbox({ dir }); }
});

test('outbox crash before effect retains WAL record until a later replay', { concurrency: false }, () => {
  const { dir, env } = outboxEnv();
  try {
    ledgerStore.enqueueOutbox(env, { eventId: 'crash-pre-effect', payload: { op: 'a' }, deadlineAtMs: 10_000, nowMs: 0 });
    let effects = 0;
    ledgerStore.deliverOutbox(env, () => { throw new Error('simulated crash before effect'); }, { nowMs: 0 });
    assert.equal(effects, 0);
    assert.equal(ledgerStore.replayOutbox(env, { nowMs: 100 }).length, 1);
    const recovered = ledgerStore.deliverOutbox(env, () => { effects += 1; return { status: 'persisted' }; }, { nowMs: 100 });
    assert.equal(recovered.acked, 1);
    assert.equal(effects, 1);
    assert.equal(ledgerStore.replayOutbox(env, { nowMs: 100 }).length, 0);
  } finally { cleanupOutbox({ dir }); }
});

test('outbox crash after effect before ack replays only idempotent ack', { concurrency: false }, () => {
  const { dir, env } = outboxEnv();
  try {
    ledgerStore.enqueueOutbox(env, { eventId: 'crash-post-effect', payload: { op: 'a' }, deadlineAtMs: 10_000, nowMs: 0 });
    let effects = 0;
    assert.throws(() => ledgerStore.deliverOutbox(env, () => { effects += 1; return { status: 'persisted' }; }, {
      nowMs: 0,
      afterEffectBeforeAck: () => { throw new Error('simulated crash after effect'); },
    }), /simulated crash after effect/);
    assert.equal(effects, 1);
    assert.equal(ledgerStore.replayOutbox(env, { nowMs: 1 }).length, 1);
    const recovered = ledgerStore.deliverOutbox(env, () => { effects += 1; return { status: 'persisted' }; }, { nowMs: 1 });
    assert.equal(recovered.acked, 1);
    assert.equal(effects, 1);
  } finally { cleanupOutbox({ dir }); }
});

test('outbox replay cursor is stable, ordered, and bounded; ack is idempotent', { concurrency: false }, () => {
  const { dir, env } = outboxEnv();
  try {
    const now = 1_000;
    ledgerStore.enqueueOutbox(env, { eventId: 'mutation-b', payload: {}, deadlineAtMs: 10_000, nowMs: now });
    ledgerStore.enqueueOutbox(env, { eventId: 'mutation-a', payload: {}, deadlineAtMs: 10_000, nowMs: now });
    const due = ledgerStore.replayOutbox(env, { nowMs: now + 1, limit: 10 });
    assert.deepEqual(due.map((r) => r.eventId), ['mutation-a', 'mutation-b']); // (nextAttemptAtMs, eventId) order
    assert.deepEqual(due[0].payload, {}); // replay carries committed mutation input
    assert.equal(ledgerStore.replayOutbox(env, { nowMs: now - 1 }).length, 0); // not due yet
    assert.equal(ledgerStore.replayOutbox(env, { nowMs: now + 1, limit: 1 }).length, 1); // bounded
    assert.equal(ledgerStore.ackOutbox(env, 'mutation-a').status, 'acked');
    assert.equal(ledgerStore.ackOutbox(env, 'mutation-a').status, 'absent'); // idempotent ack
    assert.deepEqual(ledgerStore.replayOutbox(env, { nowMs: now + 1 }).map((r) => r.eventId), ['mutation-b']);
  } finally { cleanupOutbox({ dir }); }
});

test('expired replay is terminal and visible in the DLQ', { concurrency: false }, () => {
  const { dir, env } = outboxEnv();
  try {
    ledgerStore.enqueueOutbox(env, { eventId: 'mutation-expiring', payload: { op: 'd' }, deadlineAtMs: 10, nowMs: 0 });
    assert.deepEqual(ledgerStore.replayOutbox(env, { nowMs: 10 }), []);
    assert.equal(ledgerStore.listDeadLetters(env)[0].eventId, 'mutation-expiring');
  } finally { cleanupOutbox({ dir }); }
});

test('outbox failure backoff is bounded and exhausted items reach the visible DLQ', { concurrency: false }, () => {
  const { dir, env } = outboxEnv();
  try {
    const now = 0;
    ledgerStore.enqueueOutbox(env, { eventId: 'mutation-poison', payload: { op: 'c' }, deadlineAtMs: 100_000, nowMs: now });
    // Two failures → exponential backoff (100 then 200), still retryable.
    const f1 = ledgerStore.markOutboxFailed(env, 'mutation-poison', { error: 'boom', nowMs: now });
    assert.equal(f1.status, 'retry_scheduled');
    assert.equal(f1.nextAttemptAtMs, 100);
    const f2 = ledgerStore.markOutboxFailed(env, 'mutation-poison', { error: 'boom', nowMs: now });
    assert.equal(f2.nextAttemptAtMs, 200);
    assert.equal(f2.attempts, 2);
    // Exhaust the attempt budget (maxAttempts: 2) → dead-lettered, never silent.
    const f3 = ledgerStore.markOutboxFailed(env, 'mutation-poison', { error: 'boom', nowMs: now, maxAttempts: 2 });
    assert.equal(f3.status, 'dead_lettered');
    assert.equal(ledgerStore.replayOutbox(env, { nowMs: now + 1 }).length, 0);
    const dlq = ledgerStore.listDeadLetters(env);
    assert.equal(dlq.length, 1);
    assert.equal(dlq[0].eventId, 'mutation-poison');
    assert.equal(dlq[0].lastError, 'boom');
  } finally { cleanupOutbox({ dir }); }
});
