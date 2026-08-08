import { createHash } from 'node:crypto';

const actionDefinitions = Object.freeze([
  { id: 'restart', capability: 'hub.action.restart', rollbackOrRepair: 'repair/hub/restart', fields: ['subjectId'] },
  { id: 'reconcile', capability: 'hub.action.reconcile', rollbackOrRepair: 'repair/hub/reconcile', fields: ['subjectId'] },
  { id: 'rotate-token', capability: 'hub.action.rotate-token', rollbackOrRepair: 'repair/hub/rotate-token', fields: ['subjectId'] },
  { id: 'review-proposal', capability: 'hub.action.review-proposal', rollbackOrRepair: 'repair/hub/review-proposal', fields: ['subjectId', 'decision'] },
  { id: 'restore-quarantine', capability: 'hub.action.restore-quarantine', rollbackOrRepair: 'repair/hub/restore-quarantine', fields: ['subjectId'] },
  { id: 'apply-update', capability: 'hub.action.apply-update', rollbackOrRepair: 'repair/hub/apply-update', fields: ['releaseGeneration'] },
]);

export { actionDefinitions };

const esc = value => String(value ?? 'unknown').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const safeId = value => typeof value === 'string' && /^[A-Za-z0-9._:-]{1,160}$/.test(value);
const isDigest = value => typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value);
const isRepairPath = value => typeof value === 'string' && /^repair\/hub\/[a-z-]+$/.test(value);
const RECEIPT_OUTCOMES = Object.freeze(['applied', 'rejected', 'repaired', 'rolled-back']);
const PROVEN_OUTCOMES = Object.freeze(['applied', 'repaired', 'rolled-back']);

function grants(snapshot) {
  const data = snapshot?.result?.data ?? snapshot?.data ?? snapshot ?? {};
  return Array.isArray(data.capabilityGrants) ? data.capabilityGrants : [];
}

function validPayload(action, payload) {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload) || Object.keys(payload).sort().join() !== [...action.fields].sort().join()) return false;
  if ('subjectId' in payload && !safeId(payload.subjectId)) return false;
  if ('decision' in payload && !['approve', 'reject', 'retain', 'forget'].includes(payload.decision)) return false;
  return !('releaseGeneration' in payload) || isDigest(payload.releaseGeneration);
}

// Canonical (key-sorted) JSON so the digest is stable regardless of property insertion order.
function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

function computeRequestDigest({ actionId, capability, capabilityReceiptId, confirmationNonce, payload }) {
  const material = canonicalJson({ actionId, capability, capabilityReceiptId, confirmationNonce, payload });
  return `sha256:${createHash('sha256').update(material).digest('hex')}`;
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
  const frozenPayload = Object.freeze({ ...payload });
  const requestDigest = computeRequestDigest({ actionId: action.id, capability: action.capability, capabilityReceiptId: grant.receiptId, confirmationNonce, payload: frozenPayload });
  return Object.freeze({
    state: 'awaiting-trusted-runtime', schemaVersion: 1, kind: 'hub.action.request',
    actionId: action.id, capability: action.capability, capabilityReceiptId: grant.receiptId, confirmationNonce,
    payload: frozenPayload, rollbackOrRepair: action.rollbackOrRepair, requestDigest,
  });
}

export function actionViewModel(snapshot, draft) {
  const request = buildHubActionRequest(snapshot, draft);
  return { ...request, optimisticOutcome: null, receipt: null };
}

function validReceiptShape(receipt) {
  return Boolean(receipt) && typeof receipt === 'object'
    && typeof receipt.receiptId === 'string' && receipt.receiptId.length > 0
    && typeof receipt.actionId === 'string' && receipt.actionId.length > 0
    && safeId(receipt.capabilityReceiptId)
    && isDigest(receipt.requestDigest)
    && RECEIPT_OUTCOMES.includes(receipt.outcome)
    && typeof receipt.occurredAt === 'string' && !Number.isNaN(Date.parse(receipt.occurredAt))
    && typeof receipt.runtimeIdentity === 'string' && receipt.runtimeIdentity.length > 0
    && isRepairPath(receipt.repairPath);
}

// The only path that may turn a pending request into a proven or rejected outcome. It never
// trusts a claimed outcome: the receipt must be shape-valid AND cryptographically bound (same
// actionId, capability receipt, request digest, & repair path) to the exact request that was
// issued, or the Hub renders unavailable rather than an optimistic success.
export function applyRuntimeReceipt(request, receipt) {
  if (!request || request.state !== 'awaiting-trusted-runtime') {
    return { state: 'unavailable', reason: 'no-pending-request', actionId: request?.actionId ?? 'unknown', repairPath: request?.rollbackOrRepair ?? 'unavailable' };
  }
  if (!validReceiptShape(receipt)) {
    return { state: 'unavailable', reason: 'receipt-shape-invalid', actionId: request.actionId, repairPath: request.rollbackOrRepair };
  }
  const bound = receipt.actionId === request.actionId
    && receipt.capabilityReceiptId === request.capabilityReceiptId
    && receipt.requestDigest === request.requestDigest
    && receipt.repairPath === request.rollbackOrRepair;
  if (!bound) {
    return { state: 'unavailable', reason: 'receipt-not-bound-to-request', actionId: request.actionId, repairPath: request.rollbackOrRepair };
  }
  const frozenReceipt = Object.freeze({ ...receipt });
  if (!PROVEN_OUTCOMES.includes(receipt.outcome)) {
    return { state: 'rejected', reason: 'runtime-rejected', actionId: request.actionId, receipt: frozenReceipt, repairPath: request.rollbackOrRepair };
  }
  return { state: 'proven', outcome: receipt.outcome, actionId: request.actionId, receipt: frozenReceipt, repairPath: request.rollbackOrRepair };
}

// Read-only projection: never mutates DOM state beyond innerHTML, always escapes evidence.
export function renderHubActionStatus(request, outcome, root) {
  const state = outcome?.state ?? request?.state ?? 'unavailable';
  const reason = outcome?.reason ?? request?.reason ?? (state === 'awaiting-trusted-runtime' ? 'awaiting trusted runtime execution' : 'unknown — no evidence');
  const repairPath = outcome?.repairPath ?? request?.rollbackOrRepair ?? 'unavailable';
  const receiptId = outcome?.receipt?.receiptId ?? 'none';
  root.innerHTML = `<section aria-label="Hub action status"><h3>${esc(request?.actionId)}</h3><p>State: <strong>${esc(state)}</strong> · ${esc(reason)}</p><p>Repair path: ${esc(repairPath)}</p><p>Receipt: ${esc(receiptId)}</p></section>`;
}
