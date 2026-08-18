import test from 'node:test';
import assert from 'node:assert/strict';
import { commitSalt, verifySaltCommitment, splitCorpus, verifySplit } from './holdout.mjs';

const caseIds = Array.from({ length: 40 }, (_, i) => `case-${i}`);
const salt = 'super-secret-holdout-salt-v1';

test('commitSalt is deterministic and verifySaltCommitment round-trips', () => {
  const commitment = commitSalt(salt);
  assert.match(commitment, /^[0-9a-f]{64}$/u);
  assert.equal(commitSalt(salt), commitment);
  assert.ok(verifySaltCommitment(salt, commitment));
  assert.ok(!verifySaltCommitment('wrong-salt-value-here', commitment));
});

test('splitCorpus is a pure deterministic function of (caseIds, salt, holdoutFraction)', () => {
  const a = splitCorpus({ caseIds, salt, holdoutFraction: 0.3 });
  const b = splitCorpus({ caseIds, salt, holdoutFraction: 0.3 });
  assert.deepEqual(a.assignment, b.assignment);
  assert.equal(a.splitSha256, b.splitSha256);
  assert.equal(a.visible.length + a.holdout.length, caseIds.length);
  assert.ok(a.holdout.length > 0, 'holdout set must be non-empty for a meaningful protocol');
  assert.ok(a.visible.length > 0, 'visible set must be non-empty');
});

test('a different salt produces a different split (no fixed/asserted membership)', () => {
  const a = splitCorpus({ caseIds, salt, holdoutFraction: 0.3 });
  const b = splitCorpus({ caseIds, salt: 'a-completely-different-salt', holdoutFraction: 0.3 });
  assert.notEqual(a.splitSha256, b.splitSha256);
  assert.notDeepEqual(a.assignment, b.assignment);
});

test('splitCorpus rejects duplicate ids, empty corpus, and out-of-range fraction', () => {
  assert.throws(() => splitCorpus({ caseIds: [], salt, holdoutFraction: 0.3 }));
  assert.throws(() => splitCorpus({ caseIds: ['a', 'a'], salt, holdoutFraction: 0.3 }));
  assert.throws(() => splitCorpus({ caseIds, salt, holdoutFraction: 0 }));
  assert.throws(() => splitCorpus({ caseIds, salt, holdoutFraction: 1 }));
  assert.throws(() => splitCorpus({ caseIds, salt: 'short' }));
});

test('verifySplit recomputes and confirms a genuine split (commit-reveal end to end)', () => {
  const commitment = commitSalt(salt);
  const claimed = splitCorpus({ caseIds, salt, holdoutFraction: 0.25 });
  const result = verifySplit({ caseIds, salt, holdoutCommitment: commitment, claimed });
  assert.equal(result.status, 'verified');
});

test('verifySplit fails closed when the revealed salt does not match the pre-committed commitment', () => {
  const commitment = commitSalt(salt);
  const claimed = splitCorpus({ caseIds, salt, holdoutFraction: 0.25 });
  const result = verifySplit({ caseIds, salt: 'a-different-salt-entirely', holdoutCommitment: commitment, claimed });
  assert.equal(result.status, 'mismatch');
  assert.match(result.reason, /does not match/);
});

test('verifySplit fails closed when the claimed assignment was tampered with after the fact', () => {
  const commitment = commitSalt(salt);
  const claimed = splitCorpus({ caseIds, salt, holdoutFraction: 0.25 });
  const tampered = { ...claimed, assignment: { ...claimed.assignment, [caseIds[0]]: claimed.assignment[caseIds[0]] === 'holdout' ? 'visible' : 'holdout' } };
  const result = verifySplit({ caseIds, salt, holdoutCommitment: commitment, claimed: tampered });
  assert.equal(result.status, 'mismatch');
});

test('verifySplit fails closed when the claimed split hash was tampered with', () => {
  const commitment = commitSalt(salt);
  const claimed = splitCorpus({ caseIds, salt, holdoutFraction: 0.25 });
  const tampered = { ...claimed, splitSha256: 'f'.repeat(64) };
  const result = verifySplit({ caseIds, salt, holdoutCommitment: commitment, claimed: tampered });
  assert.equal(result.status, 'mismatch');
});
