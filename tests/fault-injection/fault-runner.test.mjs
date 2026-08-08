import test from 'node:test';
import assert from 'node:assert/strict';
import { matrix, runFault, runMatrix, runRealMatrix } from './fault-runner.mjs';

const required = ['schemaVersion','kind','scenarioId','status','code','activated','observed','recovery','cleanup','timestamp','deadlineMs','inputDigest','content'];
test('all seven faults produce typed degraded/blocked/recovery receipts', () => {
  const receipts = runMatrix();
  assert.equal(receipts.length, 7);
  for (const [i, receipt] of receipts.entries()) {
    for (const key of required) assert.ok(Object.hasOwn(receipt, key), `${receipt.scenarioId}:${key}`);
    assert.equal(receipt.kind, 'membrane-fault-receipt');
    assert.equal(receipt.code, matrix.scenarios[i].expected.code);
    assert.equal(receipt.observed.healthy, false);
    assert.equal(receipt.content, null);
    assert.match(receipt.inputDigest, /^sha256:[0-9a-f]{64}$/);
  }
});
test('invalid, deadline, and cleanup faults never claim healthy', () => {
  assert.equal(runFault({ id: 'x', fault: 'unknown' }).code, 'fault_not_allowlisted');
  assert.equal(runFault(matrix.scenarios[0], { deadlineMs: 0 }).code, 'fault_deadline_invalid');
  const failed = runFault(matrix.scenarios[0], { cleanupOk: false });
  assert.equal(failed.status, 'error'); assert.equal(failed.code, 'cleanup_failed'); assert.equal(failed.recovery, null);
});
test('real Rust runtime seams produce every matrix receipt', () => {
  const receipts = runRealMatrix();
  assert.equal(receipts.length, 7);
  for (const receipt of receipts) {
    assert.equal(receipt.kind, 'membrane-fault-receipt');
    assert.equal(receipt.observed.healthy, false);
    assert.equal(receipt.cleanup, 'complete');
    assert.equal(receipt.content, null);
  }
});
