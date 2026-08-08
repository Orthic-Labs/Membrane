import test from "node:test";
import assert from "node:assert/strict";
import { signingPlan, validateReceipt, validateReleaseManifest } from "../../scripts/release/windows/contract.mjs";

const manifest = { product_version: "0.1.2", source_commit: "0123456789abcdef0123456789abcdef01234567", installer_path: "release/Membrane_0.1.2_x64-setup.exe", installer_sha256: "a".repeat(64), signing: { provider: "azure-artifact-signing", trust: "public", timestamp: "rfc3161" } };
test("manifest and credential-free signing plan are deterministic", () => { assert.equal(validateReleaseManifest(manifest), manifest); const plan = signingPlan(manifest, manifest.installer_path); assert.deepEqual(plan.verify.slice(0, 3), ["signtool", "verify", "/pa"]); assert.equal(JSON.stringify(plan).includes("SECRET"), false); assert.match(plan.args.join(" "), /Azure\.CodeSigning\.Dlib/); });
test("receipt validation fails closed on missing clean-machine gates", () => { assert.throws(() => validateReceipt({ schema: "windows-installer-receipt.v1", status: "pass", source_commit: manifest.source_commit, installer_sha256: manifest.installer_sha256, gates: { signature: "pass" } }, manifest), /install/); });
test("installer identity is exact", () => { assert.throws(() => validateReleaseManifest({ ...manifest, installer_path: "release/setup.exe" }), /installer must be/); });
