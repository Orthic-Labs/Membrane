'use strict';
// Host-issued delivery receipt — proves host loaded content natively for a session.
// The JS projection `projectDeliveryTrace` and Rust `project_delivery_trace` both
// degrade to `unavailable` when any digest is missing and to `degraded` on mismatch.

const crypto = require('node:crypto');

const DELIVERY_RECEIPT_SCHEMA = 'orthic.delivery-receipt.v1';
const DIGEST_RE = /^sha256:[a-f0-9]{64}$/;

function sha256Hex(value) {
  return crypto.createHash('sha256').update(String(value), 'utf8').digest('hex');
}
function digestOf(value) {
  return `sha256:${sha256Hex(typeof value === 'string' ? value : JSON.stringify(value))}`;
}

function buildHostDeliveryReceipt({ traceId, digest, at, mechanism } = {}) {
  const tid = String(traceId ?? '').trim();
  const d = String(digest ?? '').trim();
  if (!tid) throw new Error('traceId required');
  if (!DIGEST_RE.test(d)) throw new Error('digest must be sha256:hex');
  return {
    schema: DELIVERY_RECEIPT_SCHEMA,
    traceId: tid,
    digest: d,
    at: String(at ?? new Date().toISOString()),
    mechanism: String(mechanism ?? 'host_delivery'),
  };
}

function validateHostDeliveryReceipt(receipt) {
  if (!receipt || typeof receipt !== 'object') return null;
  if (receipt.schema !== DELIVERY_RECEIPT_SCHEMA) return null;
  const traceId = String(receipt.traceId ?? '').trim();
  const digest = String(receipt.digest ?? '').trim();
  if (!traceId || !DIGEST_RE.test(digest)) return null;
  if (receipt.at && Number.isNaN(Date.parse(String(receipt.at)))) return null;
  return { schema: DELIVERY_RECEIPT_SCHEMA, traceId, digest, at: String(receipt.at), mechanism: String(receipt.mechanism ?? 'host_delivery') };
}

function receiptDigest(receipt) {
  const norm = validateHostDeliveryReceipt(receipt);
  if (!norm) throw new Error('invalid receipt');
  return digestOf(norm);
}

module.exports = {
  DELIVERY_RECEIPT_SCHEMA,
  buildHostDeliveryReceipt,
  validateHostDeliveryReceipt,
  receiptDigest,
  digestOf,
};
