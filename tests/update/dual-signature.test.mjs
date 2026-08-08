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
console.log('MBR-911 trusted dual-signature source contract: PASS');
