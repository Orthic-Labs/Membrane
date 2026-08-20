import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { verifyReleaseEvidence } from "./verify-release-evidence.mjs";

const sha = (value) => createHash("sha256").update(value).digest("hex");
const fixture = () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-evidence-")); const file = (name, content = name) => { writeFileSync(join(root, name), content); return { path: name, sha256: sha(content) }; };
  const platform = (os, suffix) => {
    const base = { schema: "membrane.platform-acceptance.v1", receiptId: `${os}-1`, mode: "clean-vm", commit: "a".repeat(40), releaseGeneration: "c".repeat(64), version: "v1.2.3", platform: os, artifact: { name: `${os}.artifact`, sha256: "d".repeat(64) }, trust: os === "macos" ? { codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass" } : { authenticode: "pass", publicTrust: "pass", rfc3161: "pass" }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, environment: { clean: true, machineDigest: "e".repeat(64), bypassWarnings: false } };
    const contract = file(`${suffix}.contract`, JSON.stringify({ schema: base.schema, commit: base.commit, releaseGeneration: base.releaseGeneration, version: base.version, platform: base.platform, artifact: base.artifact }));
    return { os, vector_dispatch: "CORTEX_VECTOR_DISPATCH_V2", contract, receipt: file(`${suffix}.receipt`, JSON.stringify(base)) };
  };
  const artifact = file("artifact"); const trust = (kind) => ({ kind, identity: `${kind}-identity`, subject_sha256: artifact.sha256, receipt: file(`${kind}.receipt`) });
  return { root, manifest: { schema: "membrane.release-evidence.v1", release: { tag: "v1.2.3", commit: "a".repeat(40), tree: "b".repeat(40), generation: "c".repeat(64), target: "mac-arm64", artifact_sha256: artifact.sha256, vector_dispatch: "CORTEX_VECTOR_DISPATCH_V2" }, artifact, sbom: file("sbom"), provenance: file("provenance"), toolchain: file("toolchain"), tests: [file("test")], signatures: [trust("ed25519")], platform_trust: trust("apple-notary"), compatibility: file("compatibility"), install_receipts: [platform("macos", "macos"), platform("windows", "windows")], event_history: { status: "sealed", receipt: file("history") } } };
};
test("hash-bound evidence verifies", () => { const { root, manifest } = fixture(); assert.equal(verifyReleaseEvidence(manifest, root).verified, true); });
test("modified receipts and missing Windows installed proof fail closed", () => { const { root, manifest } = fixture(); manifest.sbom.sha256 = "0".repeat(64); assert.throws(() => verifyReleaseEvidence(manifest, root), /sbom hash mismatch/); const second = fixture(); second.manifest.install_receipts.pop(); assert.throws(() => verifyReleaseEvidence(second.manifest, second.root), /Windows receipts/); });
test("target trust and explicit legacy history status are enforced", () => { const { root, manifest } = fixture(); manifest.platform_trust.kind = "windows-authenticode"; assert.throws(() => verifyReleaseEvidence(manifest, root), /apple-notary/); const second = fixture(); second.manifest.event_history.status = "legacy-unsealed"; assert.equal(verifyReleaseEvidence(second.manifest, second.root).event_history, "legacy-unsealed"); });
test("evidence cannot escape root", () => { const { root, manifest } = fixture(); manifest.sbom.path = "../outside"; assert.throws(() => verifyReleaseEvidence(manifest, root), /escapes root/); });
