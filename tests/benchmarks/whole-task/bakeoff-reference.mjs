// MBR-804 — immutable reference to the already-completed vector-backend
// bakeoff (MBR-509 lineage: docs/plans/2026-08-01-vector-backend-bakeoff-harness.md,
// docs/benchmarks/vector-backend/2026-08-02-rust-vector-optimization-bakeoff-2-report.md).
//
// This module never re-runs the bakeoff: no cargo, no python harness, no
// process spawn. It only reads the five artifacts the completed bakeoff
// already produced, hashes their current bytes with plain sha256, and
// compares that against the hashes recorded here at authoring time. Any
// drift (file edited, deleted, or a rerun that regenerated a receipt) is
// reported as "mutated"/"missing", never silently accepted.
//
// The `config` hash below is independently reproducible: it equals the
// `configSha256` field recorded verbatim, and identically, in both the
// pre-existing macOS and Windows bakeoff host receipts
// (docs/benchmarks/vector-backend/round1/{mac,windows}/receipt.json), which
// this module also loads and cross-checks below.

import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Frozen at MBR-804 authoring time from the artifacts already committed by
// the completed bakeoff. Recomputed at call time from disk and compared —
// never trusted blindly, never used to regenerate a result.
export const BAKEOFF_REFERENCE = Object.freeze({
  decision: 'keep vectors inside Crypt/Membrane; implement resident in-process f32 dispatch',
  gitHead: '2f7e4c4f70d31462815155c3cd84717809db1e8a',
  artifacts: Object.freeze({
    config: Object.freeze({ path: 'engine/vector-bakeoff/config/round1-v1.json', sha256: 'ad34773d194597444f07b2a12b0ac40c647476d1483d4122a9f571ad1def6bdf' }),
    input: Object.freeze({ path: 'engine/vector-bakeoff/fixtures/smoke-v1.manifest.json', sha256: '47ac47476f38a7afbeb3a456f248c952e4e08e72aad146db195959d784fa1461' }),
    receipt: Object.freeze({ path: 'docs/benchmarks/vector-backend/2026-08-02-rust-vector-optimization-bakeoff-2-report.md', sha256: '87b2fed1917fe7497a855801dd4c84364a483aa4644331cdfcc9d04192c66ce4' }),
  }),
  hosts: Object.freeze({
    macos: Object.freeze({ path: 'docs/benchmarks/vector-backend/round1/mac/receipt.json', sha256: 'aff470ac410dd0c468d5706181665728a41766c7fe6789e64f0fae4a067f79d6' }),
    windows: Object.freeze({ path: 'docs/benchmarks/vector-backend/round1/windows/receipt.json', sha256: '0e45aaf867571825928e2ce2c70b2d9a5d40c9bcaf3277cfbc64a7b2fe5d31e7' }),
  }),
});

function sha256OfFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function checkOne(workspaceRoot, label, entry) {
  const absolute = resolve(workspaceRoot, entry.path);
  if (!existsSync(absolute)) return { label, path: entry.path, absolute, status: 'missing', expected: entry.sha256, actual: null };
  const actual = sha256OfFile(absolute);
  if (actual !== entry.sha256) return { label, path: entry.path, absolute, status: 'mutated', expected: entry.sha256, actual };
  return { label, path: entry.path, absolute, status: 'unchanged', expected: entry.sha256, actual };
}

/**
 * Reads the five reference artifacts from disk (relative to workspaceRoot),
 * verifies every one still matches its frozen sha256, and cross-checks that
 * both host receipts declare the same configSha256/inputDigest/manifestSha256
 * (proof the two hosts ran against the identical, un-rerun config/input).
 * Returns { status: 'verified' | 'mutated' | 'missing', checks, arms } and
 * never spawns a process or downloads anything.
 */
export function verifyBakeoffReference({ workspaceRoot = process.cwd() } = {}) {
  const checks = [
    checkOne(workspaceRoot, 'config', BAKEOFF_REFERENCE.artifacts.config),
    checkOne(workspaceRoot, 'input', BAKEOFF_REFERENCE.artifacts.input),
    checkOne(workspaceRoot, 'receipt', BAKEOFF_REFERENCE.artifacts.receipt),
    checkOne(workspaceRoot, 'hosts.macos', BAKEOFF_REFERENCE.hosts.macos),
    checkOne(workspaceRoot, 'hosts.windows', BAKEOFF_REFERENCE.hosts.windows),
  ];
  const missing = checks.find((c) => c.status === 'missing');
  if (missing) return { status: 'missing', reason: `${missing.label} artifact not found at ${missing.path}`, checks };
  const mutated = checks.find((c) => c.status === 'mutated');
  if (mutated) return { status: 'mutated', reason: `${mutated.label} artifact at ${mutated.path} no longer matches its recorded sha256 (bakeoff artifacts must stay immutable)`, checks };

  let macReceipt;
  let winReceipt;
  try {
    macReceipt = JSON.parse(readFileSync(resolve(workspaceRoot, BAKEOFF_REFERENCE.hosts.macos.path), 'utf8'));
    winReceipt = JSON.parse(readFileSync(resolve(workspaceRoot, BAKEOFF_REFERENCE.hosts.windows.path), 'utf8'));
  } catch (error) {
    return { status: 'mutated', reason: `host receipt is not valid JSON: ${error.message}`, checks };
  }
  for (const field of ['configSha256', 'inputDigest', 'manifestSha256', 'gitHead']) {
    if (!macReceipt[field] || macReceipt[field] !== winReceipt[field]) {
      return { status: 'mutated', reason: `host receipts disagree on ${field}; both hosts must have run the identical, un-rerun config/input`, checks };
    }
  }
  if (macReceipt.configSha256 !== BAKEOFF_REFERENCE.artifacts.config.sha256) {
    return { status: 'mutated', reason: 'host receipt configSha256 no longer matches the pinned config artifact hash', checks };
  }

  const arms = [...new Set((macReceipt.results || []).map((r) => r.runner).filter(Boolean))].sort();
  return {
    status: 'verified',
    reason: 'bakeoff config/input/receipt and both host receipts are unchanged since the completed bakeoff; the bakeoff was not rerun',
    checks,
    arms,
    declared: { configSha256: macReceipt.configSha256, inputDigest: macReceipt.inputDigest, manifestSha256: macReceipt.manifestSha256, gitHead: macReceipt.gitHead },
  };
}
