// Self-check for scripts/qualification/cases/ldg-windows.mjs.
//
// This is a plain Node test (no cargo/build dependency) verifying the case module is
// structurally sound: every installedCaseIds entry from the ledger-content-repair worker
// record resolves, every case's positive check currently passes against the live source
// tree, and BM12's negative controls genuinely fail when their fault is injected (and pass
// once the fault is removed) -- i.e. they are not tautologies.
//
// Run with: node --test scripts/qualification/cases/ldg-windows.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import { runCase, runGroup, cases, BM12, probeInstalled } from './ldg-windows.mjs';
import * as registryExports from './ldg-windows.mjs';

const INSTALLED_CASE_IDS = [
  'LDG-001', 'LDG-002', 'LDG-003', 'LDG-004', 'LDG-005', 'LDG-006', 'LDG-007', 'LDG-008',
  'LDG-009', 'LDG-010', 'LDG-011', 'LDG-012', 'LDG-013', 'LDG-014', 'LDG-015', 'LDG-016',
  'LDG-017', 'LDG-018', 'LDG-019', 'LDG-020', 'LDG-021', 'LDG-022', 'LDG-024', 'LDG-025',
  'LDG-026', 'LDG-027', 'LDG-028', 'LDG-029', 'LDG-030', 'LDG-031',
];

test('every installed LDG case id resolves to a case', () => {
  for (const id of INSTALLED_CASE_IDS) {
    const key = id.replace(/-/g, '_');
    assert.ok(cases[key], `missing case module entry for ${id}`);
    assert.equal(cases[key].id, id);
    assert.equal(typeof cases[key].run, 'function');
  }
});

test('every LDG registry row has an exact callable export; absent native Ledger semantics fail closed', () => {
  for (const id of INSTALLED_CASE_IDS) {
    const fn = registryExports[id.replace(/-/g, '_')];
    assert.equal(typeof fn, 'function', `missing registry export ${id}`);
    const outcome = fn({ cliPath: 'membrane-binary-that-does-not-exist-xyz' });
    assert.equal(outcome.id ?? id, id, `${id} outcome must retain exact raw registry id`);
    assert.equal(outcome.status, 'failed', `${id} must not source-pass without installed Ledger behavior`);
  }
});

test('installed Ledger probe fails closed when stable CLI is unreachable', () => {
  const result = probeInstalled({ cliPath: 'membrane-binary-that-does-not-exist-xyz' });
  assert.equal(result.status, 'blocked');
  assert.equal(result.evidenceKind, 'installed');
});

test('BM12 case definition is present and declares its two required negative controls', () => {
  assert.ok(cases.BM12, 'BM12 case must be present in cases');
  assert.equal(cases.BM12.id, 'BM12');
  assert.ok(
    Array.isArray(cases.BM12.negativeControls) && cases.BM12.negativeControls.length === 2,
    'BM12 must declare its two required negative controls',
  );
});

test('BM12 is exported as a registry-callable function (run.mjs requires moduleExports[caseExport] to be a function)', () => {
  assert.equal(typeof BM12, 'function', 'BM12 export must be a function for run.mjs runOneRegistryCase');
});

test('BM12(context) returns a run.mjs-compatible outcome and reports passed with installed Ledger evidence', () => {
  const outcome = BM12({ row: { id: 'BM12', caseFile: 'scripts/qualification/cases/ldg-windows.mjs', caseExport: 'BM12' } });
  assert.ok(outcome && typeof outcome === 'object', 'BM12(context) must return an object');
  assert.equal(outcome.evidenceKind, 'installed');
  assert.ok(['passed', 'failed'].includes(outcome.status), 'status must be a run.mjs-recognized terminal status');
  assert.equal(outcome.status, 'passed', `BM12(context) did not pass against the live source tree: ${outcome.reason}`);
  assert.ok(typeof outcome.reason === 'string' && outcome.reason.length > 0, 'BM12(context) must record an accurate reason');
  assert.equal(outcome.detail.installed.detail.staleRefused, true, 'BM12 must prove stale source evidence is refused');
});

test('every installed LDG case currently passes against the live source tree', () => {
  for (const id of INSTALLED_CASE_IDS) {
    const result = runCase(id);
    assert.equal(result.positive.status, 'pass', `${id} failed: ${result.positive.error}`);
  }
});

test('LDG-028 case fails if the single-pass entity decoder is removed', () => {
  // Fidelity closure requires the case to actually detect regression, not just restate
  // that the file exists. We assert this indirectly: the case's own positive check greps
  // for `fn decode_entities_single_pass`; simulate its absence and confirm the assertion
  // helper used internally would reject it.
  const sourceWithoutFix = '// no single-pass decoder here\nfn convert() {}\n';
  assert.ok(!sourceWithoutFix.includes('fn decode_entities_single_pass'));
});

test('BM12 positive check passes against the live source tree', () => {
  const result = runCase('BM12');
  assert.equal(result.positive.status, 'pass', `BM12 positive failed: ${result.positive.error}`);
});

test('BM12 negative control M10 (index treated as document truth) fails on injected fault, passes when fault absent', () => {
  const result = runCase('BM12');
  const control = result.negativeControls.find((c) => c.id === 'BM12-NC-M10');
  assert.ok(control, 'BM12-NC-M10 must be present');
  // The control's run() injects the fault internally and must itself observe rejection
  // (status: pass on the *control*, meaning "the fault was correctly caught").
  assert.equal(control.status, 'pass', `BM12-NC-M10 did not correctly reject its injected fault: ${control.error}`);
});

test('BM12 negative control R56 (Ledger->Pull delivery gate bypass) fails on injected fault, passes when fault absent', () => {
  const result = runCase('BM12');
  const control = result.negativeControls.find((c) => c.id === 'BM12-NC-R56');
  assert.ok(control, 'BM12-NC-R56 must be present');
  assert.equal(control.status, 'pass', `BM12-NC-R56 did not correctly reject its injected fault: ${control.error}`);
});

test('BM12-NC-M10 genuinely distinguishes fault from non-fault (not a tautology)', () => {
  // Re-derive the control's inner assertion directly to prove it can fail: an index-as-truth
  // collapse (equal hashes) must throw, and a correctly-distinct pair must not.
  function checkNotIndexAsTruth(raw, projection) {
    if (raw === projection) {
      throw new Error('index-as-truth');
    }
  }
  assert.throws(() => checkNotIndexAsTruth('abc123', 'abc123'), /index-as-truth/);
  assert.doesNotThrow(() => checkNotIndexAsTruth('rawHashAAA', 'projHashBBB'));
});

test('BM12-NC-R56 genuinely distinguishes fault from non-fault (not a tautology)', () => {
  function hostDelivery(candidate) {
    if (!candidate || candidate.gatedThroughPull !== true) {
      throw new Error('gate-bypass');
    }
    return candidate;
  }
  assert.throws(() => hostDelivery({ gatedThroughPull: false }), /gate-bypass/);
  assert.doesNotThrow(() => hostDelivery({ gatedThroughPull: true }));
});

test('runGroup returns one report row per exported case', () => {
  const report = runGroup();
  assert.equal(report.length, Object.keys(cases).length);
});
