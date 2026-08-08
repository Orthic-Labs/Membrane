import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../../engine/crates/membrane-updater/src/lib.rs', import.meta.url), 'utf8');
assert.match(source, /trait UpdateTrustVerifier/);
assert.match(source, /verify_tauri/);
assert.match(source, /verify_platform/);
assert.match(source, /signed_sha256 == candidate\.artifact_sha256/g);
assert.match(source, /platform_trust_valid/);
assert.match(source, /artifact_sha256/);
assert.match(source, /platform_receipt_id/);
assert.match(source, /REPAIR_PATH/);
assert.doesNotMatch(source, /pub tauri_signature_valid|pub platform_signature_valid|pub platform_notarized/);
assert.doesNotMatch(source, /std::fs::|Command::new|activate\s*\(/, 'admission must not mutate system state');

// MBR-911 (this task): downgrade admission must exist and run before either
// trust domain is consulted, and no signature bytes are ever cryptographically
// re-verified in this pure crate -- that stays the trusted adapter's job
// (Tauri's own updater plugin / codesign-spctl / signtool, wired in
// apps/membrane-hub/src-tauri/src/update_admission.rs), so this crate must
// never depend on a crypto crate.
assert.match(source, /DowngradeRejected/, 'a downgrade must have its own fail-closed failure code');
assert.match(source, /update_downgrade_rejected/);
assert.match(source, /fn is_upgrade\(/, 'downgrade admission must be a named, testable check');
assert.doesNotMatch(
  source,
  /ed25519|ring::|dalek|rsa::|Verifier::verify\(/i,
  'the pure admission crate must not hand-roll or embed signature cryptography itself',
);

console.log('MBR-911 trusted dual-signature source contract: PASS');
