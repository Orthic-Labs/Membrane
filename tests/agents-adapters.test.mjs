import test from 'node:test';
import assert from 'node:assert/strict';
import { agentsAdaptersViewModel, renderAgentsAdapters } from '../src/agents-adapters.mjs';

test('missing adapters and state evidence stay unknown', () => {
  const vm = agentsAdaptersViewModel({});
  assert.deepEqual(vm.adapters, []);
  const one = agentsAdaptersViewModel({ adapters: [{ id: 'x', client: 'codex', device: 'mac' }] }).adapters[0];
  assert.equal(one.active.state, 'unknown'); assert.equal(one.active.mechanism, 'unavailable');
});
test('client below declared capability is explicitly degraded', () => {
  const [a] = agentsAdaptersViewModel({ adapters: [{ client: 'cursor', device: 'win', declaredCapability: 'L4', clientCapability: 'L1', installed: { state: 'proven', mechanism: 'receipt', evidence: 'r1' } }] }).adapters;
  assert.equal(a.capability.state, 'degraded'); assert.match(a.capability.evidence, /L1 < declared L4/); assert.equal(a.installed.state, 'proven'); assert.equal(a.enforced.state, 'unknown');
});
test('renderer is read-only and escapes evidence', () => {
  const root = { innerHTML: '' }; renderAgentsAdapters({ adapters: [{ client: '<x>', device: 'mac', proven: 'proven' }] }, root);
  assert.match(root.innerHTML, /&lt;x&gt;/); assert.match(root.innerHTML, /proven/); assert.deepEqual(Object.keys(root), ['innerHTML']);
});
