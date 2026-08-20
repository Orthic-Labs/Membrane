import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const root = new URL('../..', import.meta.url);
const text = path => readFileSync(new URL(path, root), 'utf8');
test('team policy schema denies local-root scope & requires encrypted envelope binding', () => {
  const schema = JSON.parse(text('schemas/team-policy-sync.v1.schema.json'));
  assert.deepEqual(schema.properties.scopes.items.enum, ['tenant', 'team', 'user']);
  assert.match(schema.properties.envelope.required.join(','), /ciphertextSha256/);
  assert.equal(schema.additionalProperties, false);
});
test('runtime relies on trusted verifier, never wire encryption or authority flags', () => {
  const runtime = text('engine/crates/membrane-runtime/src/team_policy.rs');
  assert.match(runtime, /trait TeamPolicyTrustVerifier/);
  assert.match(runtime, /verifier\.verify\(policy\)/);
  assert.doesNotMatch(text('engine/crates/membrane-protocol/src/team_policy.rs'), /\b(encrypted|authorized)\b/);
  assert.match(runtime, /generation <= verification\.current_generation/);
  assert.ok(runtime.indexOf('has_valid_bounds') < runtime.indexOf('verifier.verify(policy)'));
  assert.match(runtime, /user_origin_learning_scope_preserved/);
  assert.match(runtime, /UserScopeBroadened/);
});
test('protocol exposes explicit scope boundaries & content-free receipt', () => {
  const protocol = text('engine/crates/membrane-protocol/src/team_policy.rs');
  const runtime = text('engine/crates/membrane-runtime/src/team_policy.rs');
  for (const scope of ['Tenant', 'Team', 'User', 'LocalRoot']) assert.match(protocol, new RegExp(scope));
  assert.match(protocol, /TeamPolicyReceiptV1/);
  assert.match(protocol, /offboarded_user_ids/);
  assert.match(protocol, /key_rotation_id/);
  assert.match(protocol, /audit_export_id/);
  assert.match(protocol, /offboarded_user_ids\.len\(\) <= 1_024/);
  assert.match(protocol, /self\.scopes\.len\(\) == 3/);
  assert.doesNotMatch(runtime, /format!\("\{reason:\?\}"\)/);
});
test('team sync ships opt-in-gated & off by default (MBR-1007)', () => {
  const protocol = text('engine/crates/membrane-protocol/src/team_policy.rs');
  const runtime = text('engine/crates/membrane-runtime/src/team_policy.rs');
  assert.match(protocol, /struct TeamSyncOptInV1/);
  assert.match(protocol, /pub fn disabled\(\) -> Self/);
  assert.match(runtime, /pub fn admit_team_policy_with_opt_in/);
  assert.match(runtime, /SyncNotOptedIn/);
  assert.match(runtime, /TenantOrTeamMismatch/);
  assert.match(runtime, /KeyMismatch/);
  // The opt-in gates run, and can reject, before the existing trusted-verifier path is reached.
  assert.ok(
    runtime.indexOf('opt_in.enabled') < runtime.indexOf('admit_team_policy(policy, verifier)'),
  );
});
test('cortex-store persists team sync fail-closed & never touches learning-boundary tables (MBR-1007)', () => {
  const store = text('engine/crates/cortex-store/src/team_sync.rs');
  assert.match(store, /pub fn enable_team_sync_opt_in/);
  assert.match(store, /pub fn commit_team_policy_admission/);
  assert.match(store, /pub fn team_sync_audit_log/);
  assert.match(store, /NotOptedIn/);
  assert.match(store, /TenantOrTeamMismatch/);
  assert.match(store, /KeyMismatch/);
  assert.match(store, /is_local_replay/);
  // No SQL statement in this module names an existing memory/learning-boundary table.
  for (const table of ['memories', 'memory_quarantine', 'context_feedback', 'membrane_temporal_fact']) {
    assert.doesNotMatch(store, new RegExp(`(FROM|INTO|UPDATE)\\s+${table}\\b`));
  }
  // A dedicated Rust test (schema_has_no_column_that_could_hold_key_material_or_plaintext)
  // asserts structurally, via PRAGMA table_info, that no column can hold key material or
  // plaintext secrets; this only re-confirms the assertion strings exist in that test.
  assert.match(store, /fn schema_has_no_column_that_could_hold_key_material_or_plaintext/);
});
