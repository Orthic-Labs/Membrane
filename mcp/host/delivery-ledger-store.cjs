#!/usr/bin/env node
'use strict';
// Plan 2.3: immutable JSON delivery ledger for ContextSessionV1. Lives under
// MEMBRANE_DATA_ROOT/context-delivery-ledger-v1/<sha256(ledgerKey)>/<sha256(blockId\0sourceHash)>.json.
// Raw ids and source hashes never appear on disk; only their SHA-256 digests.
// Writes use O_EXCL (`wx`) so a concurrent first writer either wins with a
// valid record or is observed as already-delivered via EEXIST. Corrupt,
// symlinked, non-regular, or path-escaping neighbours degrade to a bounded
// diagnostic — never to a crashed hook or a suppressed legitimate write.

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const os = require('node:os');

const LEDGER_SCHEMA = 'orthic.delivery-ledger-record.v1';
const DIR_NAME = 'context-delivery-ledger-v1';
const MAX_DIAG = 200;

const sha = (v) => 'sha256:' + crypto.createHash('sha256').update(String(v)).digest('hex');
const safeName = (digest) => String(digest).replace(/^sha256:/, '');
const diag = (msg) => ('delivery_ledger: ' + String(msg || 'unknown')).slice(0, MAX_DIAG);

function dataRoot(env = process.env) {
  const v = String(env.MEMBRANE_DATA_ROOT || '').trim();
  if (v) return v;
  if (process.platform === 'win32') {
    return path.join(String(env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local')), 'Membrane');
  }
  if (process.platform === 'darwin') {
    const home = String(env.HOME || '').trim();
    return path.join(home || os.homedir(), 'Library', 'Application Support', 'Membrane');
  }
  const xdg = String(env.XDG_DATA_HOME || '').trim();
  if (xdg) return path.join(xdg, 'membrane');
  const home = String(env.HOME || '').trim();
  return path.join(home || os.homedir(), '.local', 'share', 'membrane');
}
function ledgerRoot(env = process.env) { return path.join(dataRoot(env), DIR_NAME); }
function sessionDir(key, env = process.env) { return path.join(ledgerRoot(env), safeName(sha(String(key)))); }
function recordFilename(id, hash) { return safeName(sha(String(id) + '\u0000' + String(hash))); }
function recordPath(key, id, hash, env = process.env) { return path.join(sessionDir(key, env), recordFilename(id, hash)); }
function pathInside(root, target) { const r = path.resolve(root) + path.sep; const t = path.resolve(target); return t.startsWith(r); }

function readRegular(target) {
  let st;
  try { st = fs.lstatSync(target); } catch (error) { return { status: error.code === 'ENOENT' ? 'absent' : 'unreadable' }; }
  if (!st.isFile()) return { status: 'non_regular' };
  let raw; try { raw = fs.readFileSync(target, 'utf8'); } catch { return { status: 'unreadable' }; }
  let parsed; try { parsed = JSON.parse(raw); } catch { return { status: 'corrupt' }; }
  if (!parsed || parsed.schema !== LEDGER_SCHEMA) return { status: 'malformed' };
  if (typeof parsed.blockIdDigest !== 'string' || typeof parsed.sourceHashDigest !== 'string' || typeof parsed.ledgerKeyDigest !== 'string') return { status: 'malformed' };
  return { status: 'ok', record: parsed };
}

function __resetDeliveryLedgerCache() {}

function hydrate(session, ledgerKey, candidates, env = process.env) {
  const key = String(ledgerKey || '');
  if (!key || !session) return { matched: 0, diagnostic: null };
  const list = Array.isArray(candidates) ? candidates : [];
  const directory = sessionDir(key, env);
  let dirSt; try { dirSt = fs.lstatSync(directory); } catch (error) { if (error.code !== 'ENOENT') return { matched: 0, diagnostic: diag('directory_unreadable') }; return { matched: 0, diagnostic: null }; }
  if (!dirSt.isDirectory()) return { matched: 0, diagnostic: diag('entry_not_directory') };
  const matched = []; let lastDiag = null;
  for (const c of list) {
    if (!c || typeof c !== 'object') continue;
    const target = recordPath(key, c.id, c.sourceHash, env);
    if (!pathInside(directory, target)) { lastDiag = diag('path_escape'); continue; }
    const probe = readRegular(target);
    if (probe.status !== 'ok') { if (probe.status !== 'absent') lastDiag = diag(probe.status); continue; }
    if (probe.record.blockIdDigest !== sha(c.id) || probe.record.sourceHashDigest !== sha(c.sourceHash) || probe.record.ledgerKeyDigest !== sha(key)) { lastDiag = diag('mismatch'); continue; }
    session.record(c.id, probe.record.deliveryMode || 'inline', c.sourceHash, Number(probe.record.bytes) || 0);
    matched.push({ id: c.id, mode: probe.record.deliveryMode, hash: c.sourceHash, bytes: probe.record.bytes });
  }
  return { matched: matched.length, diagnostic: lastDiag };
}

function persist(session, ledgerKey, startIndex, env = process.env) {
  const key = String(ledgerKey || '');
  if (!key || !session) return { written: 0, existing: 0, diagnostic: null };
  const directory = sessionDir(key, env);
  try { fs.mkdirSync(directory, { recursive: true }); } catch { return { written: 0, existing: 0, diagnostic: diag('mkdir_failed') }; }
  const all = Array.isArray(session.delivered) ? session.delivered : [];
  const entries = all.slice(Math.max(0, Number(startIndex) || 0));
  let written = 0; let existing = 0; let lastDiag = null;
  for (const e of entries) {
    if (!e || typeof e !== 'object') continue;
    const id = String(e.id); const hash = String(e.sourceHash || '');
    const target = path.join(directory, recordFilename(id, hash));
    if (!pathInside(directory, target)) { lastDiag = diag('path_escape'); continue; }
    const record = { schema: LEDGER_SCHEMA, blockIdDigest: sha(id), sourceHashDigest: sha(hash), ledgerKeyDigest: sha(key), deliveryMode: String(e.deliveryMode || 'inline'), bytes: Number(e.bytes) || 0, deliveredAt: new Date().toISOString() };
    let fd = -1;
    try {
      fd = fs.openSync(target, 'wx', 0o600);
      fs.writeSync(fd, JSON.stringify(record));
      fs.fsyncSync(fd);
      written += 1;
    } catch (error) {
      if (error && error.code === 'EEXIST') existing += 1; else lastDiag = diag('write_failed');
    } finally { if (fd !== -1) try { fs.closeSync(fd); } catch { /* already closed */ } }
  }
  return { written, existing, diagnostic: lastDiag };
}

module.exports = { LEDGER_SCHEMA, __resetDeliveryLedgerCache, dataRoot, hydrate, ledgerRoot, persist, recordFilename, recordPath, safeName, sessionDir, identityDigest: sha };
