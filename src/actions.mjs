const actionDefinitions = Object.freeze([
  { id: 'restart', capability: 'hub.action.restart', rollbackOrRepair: 'repair/hub/restart', fields: ['subjectId'] },
  { id: 'reconcile', capability: 'hub.action.reconcile', rollbackOrRepair: 'repair/hub/reconcile', fields: ['subjectId'] },
  { id: 'rotate-token', capability: 'hub.action.rotate-token', rollbackOrRepair: 'repair/hub/rotate-token', fields: ['subjectId'] },
  { id: 'review-proposal', capability: 'hub.action.review-proposal', rollbackOrRepair: 'repair/hub/review-proposal', fields: ['subjectId', 'decision'] },
  { id: 'restore-quarantine', capability: 'hub.action.restore-quarantine', rollbackOrRepair: 'repair/hub/restore-quarantine', fields: ['subjectId'] },
  { id: 'apply-update', capability: 'hub.action.apply-update', rollbackOrRepair: 'repair/hub/apply-update', fields: ['releaseGeneration'] },
]);

export { actionDefinitions };

const safeId = value => typeof value === 'string' && /^[A-Za-z0-9._:-]{1,160}$/.test(value);
function grants(snapshot) {
  const data = snapshot?.result?.data ?? snapshot?.data ?? snapshot ?? {};
  return Array.isArray(data.capabilityGrants) ? data.capabilityGrants : [];
}

function validPayload(action, payload) {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload) || Object.keys(payload).sort().join() !== [...action.fields].sort().join()) return false;
  if ('subjectId' in payload && !safeId(payload.subjectId)) return false;
  if ('decision' in payload && !['approve', 'reject', 'retain', 'forget'].includes(payload.decision)) return false;
  return !('releaseGeneration' in payload) || /^sha256:[0-9a-f]{64}$/.test(payload.releaseGeneration);
}

// Builds an inert request only. Trusted runtime must execute it & return receipt.
export function buildHubActionRequest(snapshot, { actionId, confirmationNonce, payload = {} }) {
  const action = actionDefinitions.find(item => item.id === actionId);
  if (!action) return { state: 'unavailable', reason: 'unknown-action', actionId };
  if (typeof confirmationNonce !== 'string' || !/^[A-Za-z0-9._:-]{16,128}$/.test(confirmationNonce)) {
    return { state: 'unavailable', reason: 'confirmation-nonce-required', actionId };
  }
  const grant = grants(snapshot).find(item => item?.capability === action.capability && safeId(item.receiptId));
  if (!grant) {
    return { state: 'unavailable', reason: 'capability-not-granted', actionId, capability: action.capability };
  }
  if (!validPayload(action, payload)) return { state: 'unavailable', reason: 'action-payload-invalid', actionId };
  return Object.freeze({
    state: 'awaiting-trusted-runtime', schemaVersion: 1, kind: 'hub.action.request',
    actionId: action.id, capability: action.capability, capabilityReceiptId: grant.receiptId, confirmationNonce,
    payload: Object.freeze({ ...payload }), rollbackOrRepair: action.rollbackOrRepair,
  });
}

export function actionViewModel(snapshot, draft) {
  const request = buildHubActionRequest(snapshot, draft);
  return { ...request, optimisticOutcome: null, receipt: null };
}
