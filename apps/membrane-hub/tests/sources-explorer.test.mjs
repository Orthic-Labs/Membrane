import test from 'node:test';
import assert from 'node:assert/strict';
import { sourcesViewModel, renderSourcesExplorer } from '../src/sources.mjs';

test('source explorer preserves explicit readiness only', () => {
  const vm = sourcesViewModel({ processExists: true, readiness: { state: 'unknown', reason: 'readiness_missing' } });
  assert.equal(vm.readiness.state, 'unknown');
  assert.equal(vm.readiness.authoritative, false);
});

test('source explorer unwraps canonical operation envelope', () => {
  const vm = sourcesViewModel({ result: { data: { sourcesExplorer: { repository: { id: 'canonical' }, readiness: { state: 'ready', authoritative: true } } } } });
  assert.equal(vm.repository.id, 'canonical'); assert.equal(vm.readiness.authoritative, true);
});

test('source explorer bounds paths and neighborhoods', () => {
  const vm = sourcesViewModel({ paths: Array.from({ length: 70 }, (_, i) => ({ path: `p${i}` })), neighborhoods: Array.from({ length: 40 }, (_, i) => ({ path: `n${i}`, neighbors: [] })) });
  assert.equal(vm.paths.length, 64); assert.equal(vm.neighborhoods.length, 32);
});

test('renderer emits identity, parser, contribution, paths, and neighborhoods', () => {
  const root = { innerHTML: '' };
  renderSourcesExplorer({ repository: { id: 'repo-1' }, parser: { name: 'tree-sitter' }, recentContribution: { summary: 'latest' }, paths: [{ path: 'src/lib.rs' }], neighborhoods: [{ path: 'src/lib.rs', relation: 'imports', neighbors: ['src/main.rs'] }] }, root);
  assert.match(root.innerHTML, /repo-1/); assert.match(root.innerHTML, /tree-sitter/); assert.match(root.innerHTML, /latest/); assert.match(root.innerHTML, /src\/main\.rs/);
});
