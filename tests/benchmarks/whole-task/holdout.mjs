// MBR-804 — genuine holdout protocol.
//
// A whole-task benchmark corpus is worthless as an eval-integrity control if
// the split between "visible" (may be inspected while building/tuning the
// product) and "holdout" (only ever scored, never inspected beforehand) can
// be chosen or adjusted after someone has seen how a run performed. This
// module makes the split a pure, deterministic function of (case id, salt),
// reproducible and independently checkable by anyone holding the corpus and
// the salt, plus a commit-reveal scheme so the salt itself can be pinned
// *before* a run and only disclosed *after* results are locked — proving the
// holdout boundary was fixed in advance, not tuned against the eval set.
//
// No network, no dataset download, nothing measured here: this is pure,
// synchronous, hash-based bucketing over caller-supplied case IDs.

import { createHash } from 'node:crypto';

const HEX64 = /^[0-9a-f]{64}$/u;

function sha256Hex(input) {
  return createHash('sha256').update(input, 'utf8').digest('hex');
}

function bucketOf(caseId, salt) {
  const digest = sha256Hex(`${salt}:${caseId}`);
  // First 8 hex chars -> unsigned 32-bit int -> [0, 1) fraction. Deterministic,
  // stable across platforms/Node versions (no floating hash / Map ordering).
  const fraction = parseInt(digest.slice(0, 8), 16) / 0x100000000;
  return { digest, fraction };
}

/**
 * Publish this before any run is executed. The plaintext salt stays private
 * until results are locked; only its commitment is disclosed up front.
 */
export function commitSalt(salt) {
  if (typeof salt !== 'string' || salt.length < 8) throw new Error('salt must be a string of at least 8 characters');
  return sha256Hex(salt);
}

/** Reveal-time check: does this salt actually match the pre-published commitment? */
export function verifySaltCommitment(salt, commitment) {
  if (typeof commitment !== 'string' || !HEX64.test(commitment)) return false;
  try {
    return commitSalt(salt) === commitment;
  } catch {
    return false;
  }
}

/**
 * Deterministic split: every case ID with fraction < holdoutFraction goes to
 * "holdout", everything else to "visible". Same (caseIds, salt,
 * holdoutFraction) always produces the same split — recomputable by anyone,
 * not asserted by whoever ran the benchmark.
 */
export function splitCorpus({ caseIds, salt, holdoutFraction = 0.3 } = {}) {
  if (!Array.isArray(caseIds) || caseIds.length === 0) throw new Error('caseIds must be a non-empty array');
  if (new Set(caseIds).size !== caseIds.length) throw new Error('caseIds must be unique');
  if (typeof salt !== 'string' || salt.length < 8) throw new Error('salt must be a string of at least 8 characters');
  if (typeof holdoutFraction !== 'number' || !(holdoutFraction > 0) || !(holdoutFraction < 1)) throw new Error('holdoutFraction must be a number in (0, 1)');

  const assignment = {};
  const visible = [];
  const holdout = [];
  for (const caseId of [...caseIds].sort()) {
    const { fraction } = bucketOf(caseId, salt);
    const group = fraction < holdoutFraction ? 'holdout' : 'visible';
    assignment[caseId] = group;
    (group === 'holdout' ? holdout : visible).push(caseId);
  }

  const splitSha256 = sha256Hex(JSON.stringify({ holdoutFraction, assignment, saltCommitment: commitSalt(salt) }));
  return { saltCommitment: commitSalt(salt), holdoutFraction, assignment, visible, holdout, splitSha256 };
}

/**
 * Independent verification: recompute the split from the caseIds, the
 * revealed salt, and the claimed holdoutFraction, then compare against what
 * was claimed. Fails closed on any mismatch (wrong salt commitment, wrong
 * assignment, wrong hash) rather than trusting an asserted split.
 */
export function verifySplit({ caseIds, salt, holdoutCommitment, claimed } = {}) {
  if (!claimed || typeof claimed !== 'object') return { status: 'mismatch', reason: 'no claimed split provided' };
  if (!verifySaltCommitment(salt, holdoutCommitment)) return { status: 'mismatch', reason: 'revealed salt does not match the pre-committed holdout salt commitment' };
  if (claimed.saltCommitment !== holdoutCommitment) return { status: 'mismatch', reason: 'claimed split does not carry the pre-committed salt commitment' };

  let recomputed;
  try {
    recomputed = splitCorpus({ caseIds, salt, holdoutFraction: claimed.holdoutFraction });
  } catch (error) {
    return { status: 'mismatch', reason: `recomputation failed: ${error.message}` };
  }
  if (recomputed.splitSha256 !== claimed.splitSha256) return { status: 'mismatch', reason: 'recomputed split hash does not match the claimed split hash' };
  if (JSON.stringify(recomputed.assignment) !== JSON.stringify(claimed.assignment)) return { status: 'mismatch', reason: 'recomputed case-by-case assignment does not match the claimed assignment' };
  return { status: 'verified', reason: 'recomputed holdout split matches the claimed split and its pre-committed salt', recomputed };
}
