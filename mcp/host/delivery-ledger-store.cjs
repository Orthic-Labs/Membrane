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

const LEDGER_SCHEMA = 'membrane.delivery-ledger-record.v1';
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

// ---------------------------------------------------------------------------
// P2 §8.9 — bounded durable outbox for committed asynchronous mutations.
//
// Every committed mutation goes through ONE bounded durable outbox: a stable
// event id, attempt/deadline/backoff state, an idempotent consumer, an ack, a
// replay cursor, and an operator-visible dead-letter queue. A poisoned item is
// dead-lettered (never silently dropped) and every terminal state stays visible
// through listDeadLetters.
// ---------------------------------------------------------------------------

const OUTBOX_SCHEMA = 'membrane.mutation-outbox-record.v1';
const OUTBOX_DIR_NAME = 'mutation-outbox-v1';
const DEAD_LETTER_DIR_NAME = 'dead-letter';
const DEFAULT_MAX_ATTEMPTS = 8;
const DEFAULT_BACKOFF_BASE_MS = 100;
const DEFAULT_MAX_BACKOFF_MS = 60_000;
const DEFAULT_OUTBOX_QUEUE_DEPTH = 256;
const MAX_OUTBOX_RECORD_BYTES = 256 * 1024;
function fsyncDirectory(directory) {
  try { const fd = fs.openSync(directory, 'r'); try { fs.fsyncSync(fd); } finally { fs.closeSync(fd); } return true; } catch { return process.platform === 'win32'; }
}
function writeAllSync(fd, value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(String(value), 'utf8');
  let offset = 0;
  while (offset < bytes.length) {
    const written = fs.writeSync(fd, bytes, offset, bytes.length - offset);
    if (written <= 0) throw new Error('short outbox write');
    offset += written;
  }
}

function outboxRoot(env = process.env) { return path.join(dataRoot(env), OUTBOX_DIR_NAME); }
function deadLetterRoot(env = process.env) { return path.join(outboxRoot(env), DEAD_LETTER_DIR_NAME); }
function outboxRecordName(eventId) { return safeName(sha(String(eventId))) + '.json'; }

function readOutboxRecord(target) {
  let st;
  try { st = fs.lstatSync(target); } catch (error) { return { status: error.code === 'ENOENT' ? 'absent' : 'unreadable' }; }
  if (!st.isFile()) return { status: 'non_regular' };
  let raw; try { raw = fs.readFileSync(target, 'utf8'); } catch { return { status: 'unreadable' }; }
  let parsed; try { parsed = JSON.parse(raw); } catch { return { status: 'corrupt' }; }
  if (!parsed || parsed.schema !== OUTBOX_SCHEMA) return { status: 'malformed' };
  if (typeof parsed.eventId !== 'string' || typeof parsed.eventIdDigest !== 'string' || typeof parsed.payloadDigest !== 'string') return { status: 'malformed' };
  return { status: 'ok', record: parsed };
}

function writeOutboxRecord(root, target, record) {
  const temporary = `${target}.${process.pid}.${Date.now()}.tmp`;
  let fd = -1;
  try {
    fd = fs.openSync(temporary, 'wx', 0o600);
    writeAllSync(fd, JSON.stringify(record));
    fs.fsyncSync(fd);
    fs.closeSync(fd); fd = -1;
    fs.renameSync(temporary, target);
    return fsyncDirectory(root);
  } catch {
    try { if (fd !== -1) fs.closeSync(fd); } catch { /* best effort */ }
    try { fs.unlinkSync(temporary); } catch { /* best effort */ }
    return false;
  }
}

// Idempotent enqueue: a stable event id plus O_EXCL means a replay of the same
// commit never produces a second record. The queue is bounded by maxQueueDepth
// and each record by MAX_OUTBOX_RECORD_BYTES.
function enqueueOutbox(env = process.env, { eventId, payload, deadlineAtMs, nowMs = Date.now(), maxQueueDepth = DEFAULT_OUTBOX_QUEUE_DEPTH } = {}) {
  const id = String(eventId ?? '').trim();
  if (!id) return { status: 'invalid', reason: 'outbox_event_id_required' };
  const payloadBytes = Buffer.byteLength(JSON.stringify(payload ?? null), 'utf8');
  if (payloadBytes > MAX_OUTBOX_RECORD_BYTES) return { status: 'invalid', reason: 'outbox_payload_too_large' };
  if (!Number.isFinite(deadlineAtMs) || deadlineAtMs <= nowMs) return { status: 'invalid', reason: 'outbox_deadline_expired' };
  const root = outboxRoot(env);
  try { fs.mkdirSync(root, { recursive: true }); } catch { return { status: 'unavailable', reason: 'outbox_mkdir_failed' }; }
  const target = path.join(root, outboxRecordName(id));
  // Check an existing id before capacity. A retry must remain admissible when
  // a full queue contains its own prior WAL record, but a changed payload is
  // a conflicting mutation and must never be delivered under that id.
  const existing = readOutboxRecord(target);
  const payloadDigest = sha(payload == null ? 'null' : JSON.stringify(payload));
  if (existing.status === 'ok') {
    if (existing.record.payloadDigest !== payloadDigest) return { status: 'conflict', eventId: id, reason: 'outbox_event_id_payload_drift' };
    if (!fsyncDirectory(root)) return { status: 'unavailable', reason: 'outbox_fsync_failed' };
    return { status: 'duplicate', eventId: id };
  }
  if (existing.status !== 'absent') return { status: 'unavailable', reason: 'outbox_record_unreadable' };
  try {
    const pending = fs.readdirSync(root).filter((name) => name.endsWith('.json'));
    if (pending.length >= maxQueueDepth) return { status: 'unavailable', reason: 'outbox_queue_depth_exceeded' };
  } catch { return { status: 'unavailable', reason: 'outbox_unreadable' }; }
  const record = {
    schema: OUTBOX_SCHEMA,
    eventId: id,
    eventIdDigest: sha(id),
    payloadDigest,
    // Keep committed mutation input with its durable record. Consumers must be
    // able to replay after process loss without reconstructing an ephemeral
    // in-memory payload; the digest remains the integrity check.
    payload: payload ?? null,
    attempts: 0,
    nextAttemptAtMs: nowMs,
    deadlineAtMs: Number(deadlineAtMs),
    lastError: null,
  };
  let fd = -1;
  try {
    fd = fs.openSync(target, 'wx', 0o600);
    writeAllSync(fd, JSON.stringify(record));
    fs.fsyncSync(fd);
    if (!fsyncDirectory(root)) return { status: 'unavailable', reason: 'outbox_fsync_failed' };
    return { status: 'enqueued', eventId: id, record };
  } catch (error) {
    if (error && error.code === 'EEXIST') {
      const raced = readOutboxRecord(target);
      if (raced.status !== 'ok') return { status: 'unavailable', reason: 'outbox_record_unreadable' };
      if (raced.record.payloadDigest !== payloadDigest) return { status: 'conflict', eventId: id, reason: 'outbox_event_id_payload_drift' };
      if (!fsyncDirectory(root)) return { status: 'unavailable', reason: 'outbox_fsync_failed' };
      return { status: 'duplicate', eventId: id };
    }
    return { status: 'unavailable', reason: 'outbox_write_failed' };
  } finally { if (fd !== -1) try { fs.closeSync(fd); } catch { /* already closed */ } }
}

// Replay cursor: due records in stable (nextAttemptAtMs, eventId) order, bounded
// by limit. Consumers re-read the same cursor until they ack, so a crash never
// loses an in-flight item.
function replayOutbox(env = process.env, { nowMs = Date.now(), limit = 256 } = {}) {
  const root = outboxRoot(env);
  let names;
  try { names = fs.readdirSync(root).filter((name) => name.endsWith('.json')); } catch { return []; }
  const due = [];
  for (const name of names) {
    const probe = readOutboxRecord(path.join(root, name));
    if (probe.status !== 'ok') continue;
    if (probe.record.deadlineAtMs <= nowMs) {
      // Expired work is terminal, visible, and never replayed indefinitely.
      markOutboxFailed(env, probe.record.eventId, { nowMs });
    } else if (probe.record.nextAttemptAtMs <= nowMs) due.push(probe.record);
  }
  due.sort((a, b) => (a.nextAttemptAtMs - b.nextAttemptAtMs) || String(a.eventId).localeCompare(String(b.eventId)));
  return due.slice(0, Math.max(0, Number(limit) || 0));
}

// Ack is idempotent: an already-acked id reports absent, never a crash.
function ackOutbox(env = process.env, eventId) {
  const id = String(eventId ?? '').trim();
  if (!id) return { status: 'invalid', reason: 'outbox_event_id_required' };
  const root = outboxRoot(env);
  const target = path.join(root, outboxRecordName(id));
  if (!pathInside(root, target)) return { status: 'unavailable', reason: 'outbox_path_escape' };
  try {
    fs.unlinkSync(target);
    if (!fsyncDirectory(root)) return { status: 'unavailable', reason: 'outbox_ack_fsync_failed' };
    return { status: 'acked', eventId: id };
  } catch (error) {
    if (error && error.code === 'ENOENT') return { status: 'absent', eventId: id };
    return { status: 'unavailable', reason: 'outbox_unlink_failed' };
  }
}

// Record completion durably before acking. A process killed after an effect
// returns but before ack leaves this marker behind; replay then only performs
// the idempotent ack, never invokes that event id's effect a second time.
function markOutboxEffected(env = process.env, eventId, { nowMs = Date.now() } = {}) {
  const id = String(eventId ?? '').trim();
  if (!id) return { status: 'invalid', reason: 'outbox_event_id_required' };
  const root = outboxRoot(env);
  const target = path.join(root, outboxRecordName(id));
  const probe = readOutboxRecord(target);
  if (probe.status === 'absent') return { status: 'absent', eventId: id };
  if (probe.status !== 'ok') return { status: 'unavailable', reason: 'outbox_record_unreadable' };
  if (probe.record.effectedAtMs != null) return { status: 'effected', eventId: id };
  probe.record.effectedAtMs = Number(nowMs);
  return writeOutboxRecord(root, target, probe.record)
    ? { status: 'effected', eventId: id }
    : { status: 'unavailable', reason: 'outbox_effect_marker_failed' };
}

// A single delivery path gives all consumers identical ordering: durable WAL,
// one event-id effect, durable completion marker, then removable ack.
function deliverOutbox(env = process.env, effect, { nowMs = Date.now(), limit = 256, afterEffectBeforeAck } = {}) {
  if (typeof effect !== 'function') return { delivered: 0, failed: 0, acked: 0 };
  let delivered = 0; let failed = 0; let acked = 0;
  for (const record of replayOutbox(env, { nowMs, limit })) {
    if (record.effectedAtMs != null) {
      if (ackOutbox(env, record.eventId).status === 'acked') acked += 1;
      continue;
    }
    let outcome;
    try { outcome = effect(record.payload, record.eventId); } catch (error) { outcome = { status: 'unavailable', reason: error && error.message || 'effect_threw' }; }
    if (!outcome || outcome.status !== 'persisted') {
      markOutboxFailed(env, record.eventId, { error: outcome && outcome.reason || 'effect_failed', nowMs });
      failed += 1;
      continue;
    }
    if (markOutboxEffected(env, record.eventId, { nowMs }).status !== 'effected') {
      // Do not ack an unmarked effect: replay will retain this item for
      // operator recovery rather than silently losing a completion record.
      failed += 1;
      continue;
    }
    delivered += 1;
    if (typeof afterEffectBeforeAck === 'function') afterEffectBeforeAck(record);
    if (ackOutbox(env, record.eventId).status === 'acked') acked += 1;
  }
  return { delivered, failed, acked };
}

// Record a failed attempt with bounded exponential backoff. When the attempt
// budget or deadline is exhausted the item moves to the operator-visible DLQ;
// it is never silently dropped.
function markOutboxFailed(env = process.env, eventId, { error = 'unknown', nowMs = Date.now(), maxAttempts = DEFAULT_MAX_ATTEMPTS, backoffBaseMs = DEFAULT_BACKOFF_BASE_MS, maxBackoffMs = DEFAULT_MAX_BACKOFF_MS } = {}) {
  const id = String(eventId ?? '').trim();
  if (!id) return { status: 'invalid', reason: 'outbox_event_id_required' };
  const root = outboxRoot(env);
  const target = path.join(root, outboxRecordName(id));
  if (!pathInside(root, target)) return { status: 'unavailable', reason: 'outbox_path_escape' };
  const probe = readOutboxRecord(target);
  if (probe.status === 'absent') return { status: 'absent', eventId: id };
  if (probe.status !== 'ok') return { status: 'unavailable', reason: 'outbox_record_unreadable' };
  const record = probe.record;
  record.attempts = (Number(record.attempts) || 0) + 1;
  record.lastError = String(error).slice(0, 256);
  const exceeded = record.attempts >= maxAttempts || nowMs >= Number(record.deadlineAtMs);
  if (exceeded) {
    const dlq = deadLetterRoot(env);
    try { fs.mkdirSync(dlq, { recursive: true }); } catch { return { status: 'unavailable', reason: 'outbox_dlq_mkdir_failed' }; }
    const dlqTarget = path.join(dlq, outboxRecordName(id));
    try {
      const bytes = JSON.stringify(record);
      const fd = fs.openSync(dlqTarget, 'wx', 0o600);
      try { writeAllSync(fd, bytes); fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
      if (!fsyncDirectory(dlq)) return { status: 'unavailable', reason: 'outbox_dlq_fsync_failed' };
      fs.unlinkSync(target);
      if (!fsyncDirectory(root)) return { status: 'unavailable', reason: 'outbox_remove_fsync_failed' };
      return { status: 'dead_lettered', eventId: id, attempts: record.attempts };
    } catch (error) {
      return { status: 'unavailable', reason: error && error.code === 'EEXIST' ? 'outbox_dlq_duplicate' : 'outbox_dlq_write_failed' };
    }
  }
  const backoff = Math.min(maxBackoffMs, backoffBaseMs * Math.pow(2, record.attempts - 1));
  record.nextAttemptAtMs = nowMs + backoff;
  if (writeOutboxRecord(root, target, record)) return { status: 'retry_scheduled', eventId: id, attempts: record.attempts, nextAttemptAtMs: record.nextAttemptAtMs };
  return { status: 'unavailable', reason: 'outbox_update_failed' };
}

function listDeadLetters(env = process.env) {
  const dlq = deadLetterRoot(env);
  let names;
  try { names = fs.readdirSync(dlq).filter((name) => name.endsWith('.json')); } catch { return []; }
  const out = [];
  for (const name of names) {
    const probe = readOutboxRecord(path.join(dlq, name));
    if (probe.status === 'ok') out.push(probe.record);
  }
  return out;
}

module.exports = {
  LEDGER_SCHEMA, __resetDeliveryLedgerCache, dataRoot, hydrate, ledgerRoot, persist,
  recordFilename, recordPath, safeName, sessionDir, identityDigest: sha,
  OUTBOX_SCHEMA, outboxRoot, deadLetterRoot, enqueueOutbox, replayOutbox, ackOutbox, markOutboxEffected, deliverOutbox,
  markOutboxFailed, listDeadLetters,
};
