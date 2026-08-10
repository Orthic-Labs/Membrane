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
