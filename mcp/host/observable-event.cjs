'use strict';

const crypto = require('node:crypto');

const EVENT_TYPES = new Set([
  'user', 'assistant', 'model', 'packet_delivered', 'tool_receipt', 'tool_receipt_failed',
  'git', 'file', 'test', 'error', 'retry', 'gate', 'delegation', 'cost', 'correction',
]);
const ORIGINS = new Set(['host', 'user', 'assistant', 'tool', 'repository', 'service']);
const IDENTIFIER = /^[A-Za-z0-9._:-]{1,256}$/;
const DIGEST = /^sha256:[a-f0-9]{64}$/;
const digest = (value) => `sha256:${crypto.createHash('sha256').update(typeof value === 'string' ? value : JSON.stringify(value)).digest('hex')}`;

function buildObservableEvent({ installationId, clientId, sessionId, taskId, turnId, traceId, eventType, origin, content = '', completeness = {}, policyDigest = 'membrane-policy-v1', timestamp = new Date().toISOString(), charCount }) {
  if (!EVENT_TYPES.has(eventType)) throw new Error(`unsupported observable event type: ${eventType}`);
  if (!ORIGINS.has(origin)) throw new Error(`unsupported observable event origin: ${origin}`);
  const identifiers = [installationId, clientId, sessionId, taskId, turnId, traceId].map(String);
  if (identifiers.some((value) => !IDENTIFIER.test(value))) throw new Error('observable event identifiers must be opaque identifiers');
  const lineage = [...identifiers, eventType].join(':');
  return validateObservableEvent({
    schema: 'orthic.observable-event.v1',
    installation_id: String(installationId), client_id: String(clientId), session_id: String(sessionId),
    task_id: String(taskId), turn_id: String(turnId), trace_id: String(traceId),
    event_id: `observable-${digest(lineage).slice(7, 31)}`, event_type: eventType, origin,
    content_ref_or_digest: digest(content), timestamp,
    completeness: Object.fromEntries(Object.entries(completeness).map(([key, value]) => [key, Boolean(value)])),
    policy_snapshot_digest: digest(policyDigest),
    ...(charCount == null ? {} : { char_count: charCount }),
  });
}

function validateObservableEvent(event) {
  if (!event || typeof event !== 'object' || Array.isArray(event)) throw new Error('observable event must be an object');
  const allowed = new Set(['schema', 'installation_id', 'client_id', 'session_id', 'task_id', 'turn_id', 'trace_id', 'event_id', 'event_type', 'origin', 'content_ref_or_digest', 'timestamp', 'completeness', 'policy_snapshot_digest', 'char_count']);
  for (const key of Object.keys(event)) if (!allowed.has(key)) throw new Error(`observable event contains forbidden field: ${key}`);
  if (event.schema !== 'orthic.observable-event.v1') throw new Error('unsupported observable event schema');
  for (const key of ['installation_id', 'client_id', 'session_id', 'task_id', 'turn_id', 'trace_id', 'event_id']) {
    if (typeof event[key] !== 'string' || !IDENTIFIER.test(event[key])) throw new Error(`invalid observable event ${key}`);
  }
  if (!EVENT_TYPES.has(event.event_type) || !ORIGINS.has(event.origin)) throw new Error('unsupported observable event type or origin');
  if (event.event_type !== 'packet_delivered' && Object.hasOwn(event, 'char_count')) throw new Error('char_count is only valid for packet_delivered');
  if (event.event_type === 'packet_delivered' && event.char_count != null && (!Number.isInteger(event.char_count) || event.char_count < 0 || event.char_count > 30000)) throw new Error('char_count must be an integer between 0 and 30000');
  if (!DIGEST.test(event.content_ref_or_digest) || !DIGEST.test(event.policy_snapshot_digest)) throw new Error('observable event requires digests, not content');
  if (!event.completeness || typeof event.completeness !== 'object' || Array.isArray(event.completeness) || Object.values(event.completeness).some((value) => typeof value !== 'boolean')) {
    throw new Error('observable event completeness must contain only booleans');
  }
  if (typeof event.timestamp !== 'string' || Number.isNaN(Date.parse(event.timestamp))) throw new Error('invalid observable event timestamp');
  return event;
}

module.exports = { EVENT_TYPES: [...EVENT_TYPES], buildObservableEvent, validateObservableEvent, digest };
