import test from 'node:test';
import assert from 'node:assert/strict';
import { buildHubActionRequest } from '../../apps/membrane-hub/src/actions.mjs';

test('Hub action request binds granted capability & explicit confirmation', () => {
  const request = buildHubActionRequest({ result: { data: { capabilityGrants: [{ capability: 'hub.action.apply-update', receiptId: 'grant-update-42' }] } } }, { actionId: 'apply-update', confirmationNonce: 'confirm-update-42', payload: { releaseGeneration: `sha256:${'b'.repeat(64)}` } });
  assert.deepEqual([request.kind, request.actionId, request.capability, request.state], ['hub.action.request', 'apply-update', 'hub.action.apply-update', 'awaiting-trusted-runtime']);
  assert.equal(request.rollbackOrRepair, 'repair/hub/apply-update');
});

test('unknown requests cannot become mutations', () => {
  const request = buildHubActionRequest({ capabilityGrants: [{ capability: 'hub.action.restart', receiptId: 'grant-1' }] }, { actionId: 'delete-everything', confirmationNonce: 'confirm-delete-1' });
  assert.deepEqual(request, { state: 'unavailable', reason: 'unknown-action', actionId: 'delete-everything' });
});
