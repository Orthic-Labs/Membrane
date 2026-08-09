import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { appendObservableEvent } from './observable-ingress.cjs';

const require = createRequire(import.meta.url);
const { buildObservableEvent } = require('./observable-event.cjs');

test('observable ingress fsyncs one Membrane-owned content-free record', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-ingress-'));
  const target = path.join(directory, 'events.jsonl');
  const previous = process.env.CRYPT_TELEMETRY_INGRESS;
  process.env.CRYPT_TELEMETRY_INGRESS = target;
  try {
    const event = buildObservableEvent({
      installationId: 'install-1', clientId: 'codex', sessionId: 'session-1', taskId: 'task-1', turnId: 'turn-1', traceId: 'trace-1',
      eventType: 'tool_receipt', origin: 'tool', content: 'private command output', completeness: { receipt: true },
    });
    assert.deepEqual(appendObservableEvent(event), { status: 'persisted', target: 'membrane_prompt_ingress' });
    const record = JSON.parse(fs.readFileSync(target, 'utf8'));
    assert.deepEqual(record.observable_events[0], event);
    assert.throws(() => appendObservableEvent({ ...event, content: 'private command output' }), /forbidden field/);
  } finally {
    if (previous === undefined) delete process.env.CRYPT_TELEMETRY_INGRESS;
    else process.env.CRYPT_TELEMETRY_INGRESS = previous;
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
