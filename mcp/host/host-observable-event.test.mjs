import assert from 'node:assert/strict';
import test from 'node:test';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { EVENT_TYPES, buildObservableEvent } = require('./observable-event.cjs');

test('ObservableEventV1 factory covers every required event class and preserves lineage', () => {
  const required = ['user', 'assistant', 'model', 'packet_delivered', 'tool_receipt', 'git', 'file', 'test', 'error', 'retry', 'gate', 'delegation', 'cost', 'correction'];
  assert.deepEqual(required.filter((value) => !EVENT_TYPES.includes(value)), []);
  const event = buildObservableEvent({ installationId: 'i', clientId: 'claude_code', sessionId: 's', taskId: 't', turnId: 'u', traceId: 'x', eventType: 'correction', origin: 'user', content: 'private', completeness: { receipt: true }, timestamp: '2026-08-01T00:00:00.000Z' });
  assert.equal(event.schema, 'membrane.observable-event.v1');
  assert.match(event.event_id, /^observable-[a-f0-9]{24}$/);
  assert.match(event.content_ref_or_digest, /^sha256:[a-f0-9]{64}$/);
  assert.doesNotMatch(JSON.stringify(event), /private/);
  assert.throws(() => buildObservableEvent({ installationId: 'i', clientId: 'c', sessionId: 's', taskId: 't', turnId: 'u', traceId: 'x', eventType: 'unknown', origin: 'host' }), /unsupported observable event type/);
});
