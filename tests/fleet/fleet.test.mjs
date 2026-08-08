import test from 'node:test';
import assert from 'node:assert/strict';
import { normalizeFleet } from '../../apps/membrane-hub/src/fleet.mjs';

test('fleet projection preserves explicit observations and does not infer liveness', () => {
  const view = normalizeFleet({ payload: { fleet: { state: 'degraded', reason: 'peer evidence missing', source: 'mirror', evidence: 'cursor:7', resolver: 'read cursor', installations: [{ installation_id: 'a', state: 'unknown', reason: 'not observed', source: 'mirror', evidence: 'none', resolver: 'read mirror' }], replication: [] } } });
  assert.equal(view.state, 'degraded');
  assert.equal(view.installations.length, 1);
  assert.equal(view.installations[0].state, 'unknown');
  const absent = normalizeFleet(null);
  assert.equal(absent.state, 'unknown');
  assert.deepEqual(absent.replication, []);
  assert.deepEqual(normalizeFleet({ fleet: { installations: [{ installationId: 'b' }, { installationId: 'a' }] } }).installations.map(row => row.installationId), ['a', 'b']);
});
