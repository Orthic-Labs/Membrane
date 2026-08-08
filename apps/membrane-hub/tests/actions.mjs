import test from 'node:test';
import assert from 'node:assert/strict';
import { actionDefinitions, actionViewModel, buildHubActionRequest } from '../src/actions.mjs';

test('all post-v1 actions require a distinct capability & repair path', () => {
  assert.deepEqual(actionDefinitions.map(({ id }) => id), ['restart', 'reconcile', 'rotate-token', 'review-proposal', 'restore-quarantine', 'apply-update']);
  assert.ok(actionDefinitions.every(action => action.capability.startsWith('hub.action.') && action.rollbackOrRepair));
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
