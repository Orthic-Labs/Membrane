import test from 'node:test';
import assert from 'node:assert/strict';
import { releaseChannelViewModel, renderReleaseChannel } from '../src/release-channel.mjs';

test('missing signed evidence is unavailable, not an update', () => {
  const vm = releaseChannelViewModel({ channel: 'stable', support: { state: 'supported', startsAt: '2026-01-01' } });
  assert.equal(vm.updateEvidence, null);
  assert.match(vm.update, /unavailable/);
});

test('projection preserves channel, support, compatibility, migration, rollback', () => {
  const vm = releaseChannelViewModel({ channel: 'beta', release: '1.2.0', support: { state: 'degraded', startsAt: 's', endsAt: 'e' }, schemaCompatibility: 'migration_required', migration: 'backup then migrate', rollback: 'restore prior release' });
  assert.deepEqual([vm.channel, vm.support, vm.schemaCompatibility, vm.migration, vm.rollback], ['beta', 'degraded', 'migration_required', 'backup then migrate', 'restore prior release']);
  assert.match(vm.requiredUpdate, /unknown/);
});

test('renderer is read-only projection', () => {
  const root = { innerHTML: '' };
  renderReleaseChannel({ channel: '<nightly>', support: { state: 'unknown', startsAt: 'unknown' }, requiredUpdate: 'required' }, root);
  assert.match(root.innerHTML, /&lt;nightly&gt;/); assert.match(root.innerHTML, /missing signed update evidence/); assert.match(root.innerHTML, /required/);
});
