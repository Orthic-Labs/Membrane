import test from 'node:test';
import assert from 'node:assert/strict';
import { actionDefinitions, actionViewModel, applyRuntimeReceipt, buildHubActionRequest, renderHubActionStatus } from '../src/actions.mjs';

const snapshot = { capabilityGrants: [{ capability: 'hub.action.restart', receiptId: 'grant-restart-1' }] };
const validRequest = () => buildHubActionRequest(snapshot, { actionId: 'restart', confirmationNonce: 'confirm-restart-1', payload: { subjectId: 'local' } });
const receiptFor = (request, overrides = {}) => ({
  receiptId: 'receipt-1', actionId: request.actionId, capabilityReceiptId: request.capabilityReceiptId,
  requestDigest: request.requestDigest, outcome: 'applied', occurredAt: '2026-08-08T00:00:00.000Z',
  runtimeIdentity: 'trusted-runtime-1', repairPath: request.rollbackOrRepair, ...overrides,
});

test('all post-v1 actions require a distinct capability & repair path', () => {
  assert.deepEqual(actionDefinitions.map(({ id }) => id), ['restart', 'reconcile', 'rotate-token', 'review-proposal', 'restore-quarantine', 'apply-update']);
  assert.ok(actionDefinitions.every(action => action.capability.startsWith('hub.action.') && action.rollbackOrRepair));
  assert.equal(new Set(actionDefinitions.map(a => a.capability)).size, actionDefinitions.length);
  assert.equal(new Set(actionDefinitions.map(a => a.rollbackOrRepair)).size, actionDefinitions.length);
});

test('builder never dispatches or claims success', () => {
  const request = buildHubActionRequest({ capabilityGrants: [{ capability: 'hub.action.restart', receiptId: 'grant-restart-1' }] }, { actionId: 'restart', confirmationNonce: 'confirm-restart-1', payload: { subjectId: 'local' } });
  assert.equal(request.state, 'awaiting-trusted-runtime');
  assert.equal(request.confirmationNonce, 'confirm-restart-1');
  assert.equal(request.capabilityReceiptId, 'grant-restart-1');
  assert.equal(request.payload.subjectId, 'local');
  assert.equal('dispatch' in request, false);
  assert.equal('outcome' in request, false);
});

test('missing nonce or capability stays unavailable without UI optimism', () => {
  assert.equal(buildHubActionRequest({}, { actionId: 'restart', confirmationNonce: 'confirm-restart-1', payload: { subjectId: 'local' } }).reason, 'capability-not-granted');
  const view = actionViewModel({ capabilityGrants: [{ capability: 'hub.action.restart', receiptId: 'grant-1' }] }, { actionId: 'restart', confirmationNonce: '' });
  assert.deepEqual([view.state, view.reason, view.optimisticOutcome, view.receipt], ['unavailable', 'confirmation-nonce-required', null, null]);
});

test('action payloads are exact & bounded', () => {
  const snapshot = { capabilityGrants: [{ capability: 'hub.action.apply-update', receiptId: 'grant-update-1' }] };
  assert.equal(buildHubActionRequest(snapshot, { actionId: 'apply-update', confirmationNonce: 'confirm-update-1', payload: { releaseGeneration: `sha256:${'a'.repeat(64)}` } }).state, 'awaiting-trusted-runtime');
  assert.equal(buildHubActionRequest(snapshot, { actionId: 'apply-update', confirmationNonce: 'confirm-update-1', payload: { releaseGeneration: 'latest' } }).reason, 'action-payload-invalid');
});

test('every request carries a stable digest the runtime receipt must prove against', () => {
  const a = validRequest();
  const b = validRequest();
  assert.match(a.requestDigest, /^sha256:[0-9a-f]{64}$/);
  assert.equal(a.requestDigest, b.requestDigest, 'identical requests digest identically');
  const differentNonce = buildHubActionRequest(snapshot, { actionId: 'restart', confirmationNonce: 'confirm-restart-2', payload: { subjectId: 'local' } });
  assert.notEqual(a.requestDigest, differentNonce.requestDigest, 'digest binds the exact confirmation nonce');
});

test('a shape-valid, correctly bound receipt is the only way to reach proven', () => {
  const request = validRequest();
  const outcome = applyRuntimeReceipt(request, receiptFor(request));
  assert.equal(outcome.state, 'proven');
  assert.equal(outcome.outcome, 'applied');
  assert.equal(outcome.repairPath, 'repair/hub/restart');
  assert.ok(Object.isFrozen(outcome.receipt));
});

test('a runtime-rejected outcome never becomes a proven success', () => {
  const request = validRequest();
  const outcome = applyRuntimeReceipt(request, receiptFor(request, { outcome: 'rejected' }));
  assert.equal(outcome.state, 'rejected');
  assert.notEqual(outcome.state, 'proven');
  assert.equal(outcome.repairPath, 'repair/hub/restart');
});

test('a receipt bound to a different request, action, or repair path is rejected as unproven', () => {
  const request = validRequest();
  const tamperedDigest = applyRuntimeReceipt(request, receiptFor(request, { requestDigest: `sha256:${'0'.repeat(64)}` }));
  assert.deepEqual([tamperedDigest.state, tamperedDigest.reason], ['unavailable', 'receipt-not-bound-to-request']);
  const wrongRepairPath = applyRuntimeReceipt(request, receiptFor(request, { repairPath: 'repair/hub/reconcile' }));
  assert.deepEqual([wrongRepairPath.state, wrongRepairPath.reason], ['unavailable', 'receipt-not-bound-to-request']);
  const malformed = applyRuntimeReceipt(request, { ...receiptFor(request), outcome: 'made-up' });
  assert.deepEqual([malformed.state, malformed.reason], ['unavailable', 'receipt-shape-invalid']);
  assert.equal(applyRuntimeReceipt(request, null).reason, 'receipt-shape-invalid');
});

test('a receipt can never be applied to a request that was never granted', () => {
  const denied = buildHubActionRequest({}, { actionId: 'restart', confirmationNonce: 'confirm-restart-1', payload: { subjectId: 'local' } });
  const outcome = applyRuntimeReceipt(denied, receiptFor({ actionId: 'restart', capabilityReceiptId: 'grant-restart-1', requestDigest: `sha256:${'1'.repeat(64)}`, rollbackOrRepair: 'repair/hub/restart' }));
  assert.deepEqual([outcome.state, outcome.reason], ['unavailable', 'no-pending-request']);
});

test('renderer is read-only, escapes evidence, & never renders unproven success', () => {
  const request = validRequest();
  const root = { innerHTML: '' };
  renderHubActionStatus({ ...request, actionId: '<restart>' }, applyRuntimeReceipt(request, receiptFor(request, { outcome: 'rejected' })), root);
  assert.match(root.innerHTML, /&lt;restart&gt;/);
  assert.match(root.innerHTML, /rejected/);
  assert.doesNotMatch(root.innerHTML, /proven/);
  assert.deepEqual(Object.keys(root), ['innerHTML']);
  const pending = { innerHTML: '' };
  renderHubActionStatus(request, null, pending);
  assert.match(pending.innerHTML, /awaiting-trusted-runtime/);
});
