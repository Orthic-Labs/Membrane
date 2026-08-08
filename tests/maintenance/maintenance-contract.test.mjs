import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../../engine/crates/membrane-supervisor/src/maintenance.rs', import.meta.url), 'utf8');

test('MBR-508 keeps maintenance planning pure & bounded', () => {
  for (const token of ['Consolidation', 'ContradictionDetection', 'IndexMaintenance', 'ProposalCreation', 'budget_units', 'deadline_unix_ms', 'cancelled']) {
    assert.match(source, new RegExp(token));
  }
  assert.match(source, /pub fn plan<V: MaintenanceAuthorityVerifier>/);
  assert.match(source, /verifier\.verify\(&request\.authority\)/);
  assert.match(source, /AuthoritySelfCertified/);
  assert.match(source, /AuthorityScopeDenied/);
  assert.match(source, /issuer_id == request\.scheduler_id/);
  assert.match(source, /MAX_MAINTENANCE_BUDGET_UNITS|MAX_MAINTENANCE_WINDOW_MS/);
  assert.match(source, /allowed_kinds\.contains\(&request\.kind\)/);
  assert.doesNotMatch(source, /pub verified: bool/);
  assert.doesNotMatch(source, /std::thread|tokio::spawn|std::fs::|rusqlite|Store/);
});

test('MBR-508 receipts exclude user content & execution output', () => {
  const receipt = source.slice(source.indexOf('pub struct MaintenanceReceipt'), source.indexOf('pub struct MaintenancePlan'));
  assert.match(receipt, /schema_version/);
  assert.match(receipt, /authority_receipt_id/);
  for (const forbidden of ['content', 'document', 'embedding', 'path', 'result']) {
    assert.doesNotMatch(receipt, new RegExp(forbidden, 'i'));
  }
});
