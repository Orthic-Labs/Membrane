import test from 'node:test';
import assert from 'node:assert/strict';
import { memorySentinelViewModel, renderMemorySentinel } from '../src/memory-sentinel.mjs';

test('missing evidence stays unknown and lists are bounded', () => {
  const vm = memorySentinelViewModel({ processExists: true, proposals: Array.from({ length: 70 }, (_, i) => String(i)) });
  assert.equal(vm.evidence.state, 'unknown'); assert.equal(vm.gate.authoritative, false); assert.equal(vm.lifecycle.active, null); assert.equal(vm.proposals.ids.length, 64);
});
test('unwraps report and preserves lifecycle counts', () => {
  const vm = memorySentinelViewModel({ result: { data: { lifecycle: { active: 3, demoted: 2 }, evidence: { state: 'valid', valid: true }, gate: { state: 'pass', authoritative: true } } } });
  assert.equal(vm.lifecycle.active, 3); assert.equal(vm.lifecycle.demoted, 2); assert.equal(vm.evidence.valid, true); assert.equal(vm.gate.state, 'pass');
});
test('renderer is read-only and renders lifecycle, expiry, criteria, gate', () => {
  const root = { innerHTML: '' }; renderMemorySentinel({ lifecycle: { active: 1 }, scratchpad: { count: 2 }, taskCriteria: { count: 1 }, gate: { state: 'blocked' } }, root);
  assert.match(root.innerHTML, /Memory Sentinel/); assert.match(root.innerHTML, /Scratchpad expiry/); assert.match(root.innerHTML, /Task criteria/); assert.match(root.innerHTML, /blocked/); assert.deepEqual(Object.keys(root), ['innerHTML']);
});
