import assert from 'node:assert/strict';
import test from 'node:test';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { buildHostDeliveryReceipt, validateHostDeliveryReceipt, DELIVERY_RECEIPT_SCHEMA } = require('./delivery-receipt.cjs');

test('host delivery receipt schema is stable', () => {
  assert.equal(DELIVERY_RECEIPT_SCHEMA, 'orthic.delivery-receipt.v1');
});

test('build and validate round-trip', () => {
  const r = buildHostDeliveryReceipt({ traceId: 't-1', digest: `sha256:${'a'.repeat(64)}` });
  assert.equal(validateHostDeliveryReceipt(r).traceId, 't-1');
  assert.equal(validateHostDeliveryReceipt({ schema: 'wrong', traceId: 't', digest: r.digest }), null);
});

test('invalid digest is rejected', () => {
  assert.throws(() => buildHostDeliveryReceipt({ traceId: 't', digest: 'bad' }), /digest must be/);
  assert.equal(validateHostDeliveryReceipt({ schema: DELIVERY_RECEIPT_SCHEMA, traceId: 't', digest: 'bad' }), null);
});
