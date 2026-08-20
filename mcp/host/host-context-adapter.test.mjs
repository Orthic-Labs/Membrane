import assert from 'node:assert/strict';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import test from 'node:test';
import path from 'node:path';

const require = createRequire(import.meta.url);
const adapter = require('./context-adapter.cjs');
const observable = require('./observable-event.cjs');
const fakeClient = fileURLToPath(new URL('./fixtures/membrane-client.mjs', import.meta.url));

test('host adapter emits a typed client identity, never a gateway-derived alias', () => {
  // Plan convention 3: the identity is one of claude_code | codex | mcp |
  // api_worker | other. It used to be derived from ANTHROPIC_BASE_URL, which
  // produced 'ccx'/'host-adapter' — deployment details, not client identities,
  // and neither is in Membrane's SELF_LOADING_RULE_CLIENTS, so the rules
  // capability split silently never engaged.
  const TYPED = ['claude_code', 'codex', 'mcp', 'api_worker', 'other'];
  assert.ok(TYPED.includes(adapter.defaultClient({ ANTHROPIC_BASE_URL: 'http://127.0.0.1:8801' })));
  assert.equal(adapter.defaultClient({ ANTHROPIC_BASE_URL: 'http://127.0.0.1:8801' }), 'claude_code');
  assert.equal(adapter.defaultClient({ ANTHROPIC_BASE_URL: 'https://api.anthropic.com' }), 'claude_code');
  assert.equal(adapter.defaultClient({ CODEX_THREAD_ID: 'thread-1' }), 'codex');
  // An explicit typed MEMBRANE_CLIENT wins; an untyped one degrades to 'other'
  // rather than leaking a new ad-hoc string into telemetry.
  assert.equal(adapter.defaultClient({ MEMBRANE_CLIENT: 'mcp' }), 'mcp');
  assert.equal(adapter.defaultClient({ MEMBRANE_CLIENT: 'ccx' }), 'other');
});

test('packet delivery char count is bounded, packet-only, and content-free', () => {
  const event = observable.buildObservableEvent({
    installationId: 'installation-1', clientId: 'codex', sessionId: 'session-1', taskId: 'task-1', turnId: 'turn-1', traceId: 'trace-1',
    eventType: 'packet_delivered', origin: 'host', content: 'private content', charCount: 42,
  });
  assert.equal(event.char_count, 42);
  assert.throws(() => observable.validateObservableEvent({ ...event, char_count: 30001 }), /char_count/);
  assert.throws(() => observable.validateObservableEvent({ ...event, event_type: 'tool_receipt' }), /packet_delivered/);
});

test('host adapter emits bounded data-only delivery heartbeat for canonical request', { concurrency: false }, () => {
  const root = tmpdir();
  const request = adapter.buildRequest({ event: 'UserPromptSubmit', prompt: 'inspect current graph', session_id: 'session-1', turn_id: 'turn-1' }, root);
  assert.equal(request.taskEnvelope.schema, 'membrane.task-envelope.v1');
  assert.equal(request.turnEnvelope.schema, 'membrane.turn-envelope.v1');
  const result = adapter.runClient(request, root, { client: fakeClient });
  assert.equal(result.state, 'context_enforced');
  const rendered = adapter.render(result);
  assert.match(rendered, /Membrane: degraded/);
  assert.match(rendered, /packet_delivered/);
  assert.match(rendered, /dataOnly/);
});

test('host adapter renders visible degraded state when client is unavailable', { concurrency: false }, () => {
  const prior = process.env.MEMBRANE_CONTEXT_CLIENT;
  process.env.MEMBRANE_CONTEXT_CLIENT = '/missing/membrane-client.mjs';
  try {
    const result = adapter.runClient(adapter.buildRequest({ task: 'x' }, '/tmp'), '/tmp');
    assert.equal(result.state, 'degraded');
    assert.match(adapter.render(result), /Membrane: degraded/);
  } finally {
    if (prior === undefined) delete process.env.MEMBRANE_CONTEXT_CLIENT;
    else process.env.MEMBRANE_CONTEXT_CLIENT = prior;
  }
});

test('ccx context stays bound to this installed workspace when profile cwd drifts', { concurrency: false }, () => {
  // The hook binds to its own installed workspace (path.resolve(__dirname,'..','..')),
  // which is platform-specific — /Volumes/D/claude on Mac, D:\Claude on Windows.
  const expectedRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
  assert.equal(adapter.resolveWorkspaceRoot({ cwd: '/Users/adrdsouza/ClaudeProfiles/claudecodex-profile' }), expectedRoot);
});

// G1 regression: render() used to serialize the whole packet -- block `text`
// included -- into one JSON blob, so content reached the model while every block
// still reported deliveryStage "planned" and deliveredChars 0. Delivery has to be
// finalized and accounted, and bounded by the packet budget.
test('selected content is rendered into the prompt and accounted as finalized', () => {
  const packet = {
    schema: 'orthic.context-packet.v1',
    budget: { packetCharBudgetDefault: 30000, configuredPacketCharBudget: 30000 },
    blocks: [
      { id: 'rules:AGENTS.md', provider: 'rules', priority: 40, estimatedTokens: 10, resolver: 'read AGENTS.md', text: 'ALPHA-CONTENT-MARKER' },
      { id: 'git:meta:branch', provider: 'git', priority: 10, estimatedTokens: 2, resolver: 'git rev-parse', text: 'BRAVO-CONTENT-MARKER' },
    ],
  };
  const rendered = adapter.render({ state: 'context_enforced', payload: { packet, providerStatus: 'ready', receipts: [] } });

  // The content is actually in the prompt.
  assert.match(rendered, /ALPHA-CONTENT-MARKER/);
  assert.match(rendered, /BRAVO-CONTENT-MARKER/);
  // ...and it is labelled as data, not instruction.
  assert.match(rendered, /instructionPolicy="data_only"/);
  // ...and the header reports a non-zero delivered count.
  assert.match(rendered, /\ndelivered: [1-9]\d* chars\n/);

  for (const block of packet.blocks) {
    assert.equal(block.deliveryStage, 'finalized');
    assert.equal(block.deliveryClass, 'rendered');
    assert.equal(block.dropReason, 'none');
    assert.ok(block.deliveredChars > 0, `${block.id} delivered 0 chars`);
  }
  // Per-provider accounting sums to the blocks it covers.
  assert.equal(packet.providerAccounting.rules.deliveredChars, packet.blocks[0].deliveredChars);
  assert.equal(packet.providerAccounting.git.deliveredChars, packet.blocks[1].deliveredChars);
  // The data block ships metadata only; content must not be duplicated.
  const data = JSON.parse(/<membrane-context-data>(.*)<\/membrane-context-data>/s.exec(rendered)[1]);
  assert.ok(data.packet.blocks.every((block) => block.text === undefined));
});

test('prepared delivery supplies one finalized count to rendering', () => {
  const packet = { budget: { packetCharBudgetDefault: 30000, configuredPacketCharBudget: 30000 }, blocks: [{ id: 'kept', provider: 'rules', resolver: 'read', text: 'COUNT-MARKER' }] };
  const result = { state: 'context_enforced', payload: { packet, providerStatus: 'ready', receipts: [] } };
  const prepared = adapter.prepareDelivery(result);
  assert.ok(prepared.delivery.deliveredChars > 0);
  assert.match(adapter.render(result, prepared), new RegExp(`delivered: ${prepared.delivery.deliveredChars} chars`));
});

test('zero-delivery response retains bounded data-only receipt metadata', () => {
  const packet = {
    schema: 'orthic.context-packet.v1',
    budget: { packetCharBudgetDefault: 30000, configuredPacketCharBudget: 30000 },
    blocks: [{ id: 'empty', provider: 'rules', priority: 1, resolver: 'read empty', text: '' }],
  };
  const rendered = adapter.render({ state: 'context_enforced', payload: { packet, providerStatus: 'ready', receipts: [] } });

  assert.match(rendered, /Membrane: degraded/);
  assert.match(rendered, /\ndelivered: 0 chars\n/);
  assert.match(rendered, /omissions: zero_delivery/);
  assert.doesNotMatch(rendered, /<membrane-context instructionPolicy=/);
  const match = /<membrane-context-data>(.*)<\/membrane-context-data>/s.exec(rendered);
  assert.ok(match, 'zero-delivery response must retain data-only receipt metadata');
  const data = JSON.parse(match[1]);
  assert.equal(data.event, 'packet_delivered');
  assert.equal(data.dataOnly, true);
  assert.ok(data.packet.blocks.every((block) => block.text === undefined));
});

test('delivery stops at the packet budget instead of overrunning the prompt', () => {
  const packet = {
    budget: { packetCharBudgetDefault: 30000, configuredPacketCharBudget: 120 },
    blocks: [
      { id: 'kept', provider: 'rules', priority: 90, resolver: 'read a', text: 'K'.repeat(60) },
      { id: 'dropped', provider: 'rules', priority: 10, resolver: 'read b', text: 'D'.repeat(400) },
    ],
  };
  adapter.finalize(packet, 30000);
  const [kept, dropped] = packet.blocks;
  assert.equal(kept.dropReason, 'none');
  assert.ok(kept.deliveredChars > 0);
  assert.equal(dropped.dropReason, 'packet_budget_exceeded');
  assert.equal(dropped.deliveredChars, 0);
  assert.equal(dropped.deliveryClass, 'resolver_backed');
  const total = packet.blocks.reduce((sum, block) => sum + block.deliveredChars, 0);
  assert.ok(total <= packet.budget.effectivePacketCharBudget, `${total} > budget`);
});
