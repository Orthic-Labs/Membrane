import test from 'node:test';
import assert from 'node:assert/strict';
import { applyRuntimeReceipt, buildHubActionRequest } from '../../apps/membrane-hub/src/actions.mjs';

test('Hub action request binds granted capability & explicit confirmation', () => {
  const request = buildHubActionRequest({ result: { data: { capabilityGrants: [{ capability: 'hub.action.apply-update', receiptId: 'grant-update-42' }] } } }, { actionId: 'apply-update', confirmationNonce: 'confirm-update-42', payload: { releaseGeneration: `sha256:${'b'.repeat(64)}` } });
  assert.deepEqual([request.kind, request.actionId, request.capability, request.state], ['hub.action.request', 'apply-update', 'hub.action.apply-update', 'awaiting-trusted-runtime']);
  assert.equal(request.rollbackOrRepair, 'repair/hub/apply-update');
  assert.match(request.requestDigest, /^sha256:[0-9a-f]{64}$/);
});

test('unknown requests cannot become mutations', () => {
  const request = buildHubActionRequest({ capabilityGrants: [{ capability: 'hub.action.restart', receiptId: 'grant-1' }] }, { actionId: 'delete-everything', confirmationNonce: 'confirm-delete-1' });
  assert.deepEqual(request, { state: 'unavailable', reason: 'unknown-action', actionId: 'delete-everything' });
});

test('repair path survives every outcome: a rejected or unbound receipt still surfaces the exact repair route', () => {
  const request = buildHubActionRequest({ capabilityGrants: [{ capability: 'hub.action.restore-quarantine', receiptId: 'grant-quarantine-1' }] }, { actionId: 'restore-quarantine', confirmationNonce: 'confirm-quarantine-1', payload: { subjectId: 'item-1' } });
  const rejected = applyRuntimeReceipt(request, {
    receiptId: 'receipt-9', actionId: request.actionId, capabilityReceiptId: request.capabilityReceiptId,
    requestDigest: request.requestDigest, outcome: 'rejected', occurredAt: '2026-08-08T00:00:00.000Z',
    runtimeIdentity: 'trusted-runtime-1', repairPath: request.rollbackOrRepair,
  });
  assert.deepEqual([rejected.state, rejected.repairPath], ['rejected', 'repair/hub/restore-quarantine']);
  const forged = applyRuntimeReceipt(request, {
    receiptId: 'receipt-9', actionId: request.actionId, capabilityReceiptId: 'grant-quarantine-1',
    requestDigest: `sha256:${'f'.repeat(64)}`, outcome: 'applied', occurredAt: '2026-08-08T00:00:00.000Z',
    runtimeIdentity: 'trusted-runtime-1', repairPath: request.rollbackOrRepair,
  });
  assert.deepEqual([forged.state, forged.reason, forged.repairPath], ['unavailable', 'receipt-not-bound-to-request', 'repair/hub/restore-quarantine']);
});
