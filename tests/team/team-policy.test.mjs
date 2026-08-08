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
