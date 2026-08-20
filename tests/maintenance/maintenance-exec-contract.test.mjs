import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(
  new URL('../../engine/crates/cortex-store/src/maintenance_exec.rs', import.meta.url),
  'utf8',
);

test('MBR-508 store-side execution refuses to self-certify authority', () => {
  assert.match(source, /MissingAuthority/);
  assert.match(source, /authority_receipt_id\.trim\(\)\.is_empty\(\)/);
  // No verifier/verification logic lives in the execution module -- only the upstream
  // Membrane Hub's resident planner grants authority_receipt_id upstream.
  assert.doesNotMatch(source, /trait\s+\w*Verif/);
  assert.doesNotMatch(source, /fn\s+verify\(/);
});

test('MBR-508 store-side execution is bounded, cancellable & transactional', () => {
  for (const token of [
    'MaintenanceExecOutcome',
    'RolledBackCancelled',
    'RolledBackDeadlineExpired',
    'RolledBackBudgetExhausted',
    'RolledBackUnitFailed',
    'is_cancelled',
    'budget_units',
    'deadline_unix_ms',
    'TransactionBehavior::Immediate',
  ]) {
    assert.match(source, new RegExp(token));
  }
  // Only a fully-completed pass commits; every other outcome must drop (never commit) the tx.
  assert.match(source, /MaintenanceExecOutcome::Committed\s*=>\s*tx\.commit\(\)/);
});

test('MBR-508 execution receipts exclude user content & are schema-versioned', () => {
  const receipt = source.slice(
    source.indexOf('pub struct MaintenanceExecReceipt'),
    source.indexOf('pub struct MaintenanceExecReceipt') + 600,
  );
  assert.match(receipt, /schema_version/);
  assert.match(receipt, /authority_receipt_id/);
  assert.match(receipt, /outcome/);
  for (const forbidden of ['content', 'document', 'embedding', 'path', 'result']) {
    assert.doesNotMatch(receipt, new RegExp(forbidden, 'i'));
  }
});
