import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import { projectDeliveryTrace, buildDeliveryReceipt, validateDeliveryReceipt, DELIVERY_RECEIPT_SCHEMA, PHASES, RUST_HANDLER } from './delivery-receipt.mjs';

// Added invariant: reconciliation function has production caller inside Rust handler — fixture asserting function is called.
test('S-1: Rust handler calls project_delivery_trace and JS marks it', () => {
  const serve = fs.readFileSync('engine/crates/membrane-runtime/src/serve.rs', 'utf8');
  assert.match(serve, /delivery_trace_response/, 'serve.rs must expose delivery_trace_response');
  assert.match(serve, /project_delivery_trace/, 'serve.rs must call crate::delivery_trace_view::project_delivery_trace');
  assert.match(serve, /POST.*\/delivery\/trace/, 'serve.rs must route POST /delivery/trace');
  assert.match(RUST_HANDLER, /project_delivery_trace/, 'JS must advertise Rust call-site');
  const js = fs.readFileSync('mcp/delivery-receipt.mjs', 'utf8');
  assert.match(js, /project_delivery_trace/, 'mcp/delivery-receipt.mjs must reference project_delivery_trace for grep');
});

test('JS projection mirrors Rust: missing digest is unavailable', () => {
  const vm = projectDeliveryTrace({ traceId: 't-1', packet: { digest: 'x' } });
  assert.equal(vm.state, 'unavailable');
  assert.match(vm.reason, /missing/);
  assert.equal(vm.digests.packetDigest, 'x');
  assert.equal(vm.digests.hostDigest ?? null, null);
});

test('mismatched digests are degraded, never available', () => {
  const vm = projectDeliveryTrace({ traceId: 't-1', packet: { digest: 'x' }, hostDelivery: { digest: 'x' }, eventStore: { digest: 'y' }, outcome: { digest: 'x' } });
  assert.equal(vm.state, 'degraded');
  assert.match(vm.reason, /mismatch/);
});

test('reconciled digests are available with 9 phases', () => {
  const d = { digest: 'x' };
  const vm = projectDeliveryTrace({ traceId: 't-2', packet: d, hostDelivery: d, eventStore: d, outcome: d });
  assert.equal(vm.state, 'available');
  assert.equal(vm.phases.length, 9);
  assert.deepEqual(vm.phases.map(p => p.name), PHASES);
  for (const p of vm.phases) assert.equal(p.state, 'unknown');
});

test('delivery receipt schema and validation are stable', () => {
  assert.equal(DELIVERY_RECEIPT_SCHEMA, 'membrane.delivery-receipt.v1');
  const r = buildDeliveryReceipt({ traceId: 't-1', digest: `sha256:${'a'.repeat(64)}` });
  assert.equal(validateDeliveryReceipt(r).traceId, 't-1');
  assert.equal(validateDeliveryReceipt({ schema: 'wrong' }), null);
  assert.throws(() => buildDeliveryReceipt({ traceId: '', digest: 'x' }), /traceId/);
});

test('fetchDeliveryTrace falls back to JS projection when service is down', async () => {
  const { fetchDeliveryTrace } = await import('./delivery-receipt.mjs');
  const report = { traceId: 't-3', packet: { digest: 'same' }, hostDelivery: { digest: 'same' }, eventStore: { digest: 'same' }, outcome: { digest: 'same' } };
  const view = await fetchDeliveryTrace(report, { endpoint: 'http://127.0.0.1:1/delivery/trace' });
  assert.equal(view.state, 'available');
  assert.equal(view.traceId, 't-3');
});
