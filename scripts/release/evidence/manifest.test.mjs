import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { verifyReleaseEvidence } from "../verify-release-evidence.mjs";
import { assembleReleaseEvidenceManifest, buildAddonEvidence, buildProvenance, buildToolchain, readAddonManifest, RELEASE_EVIDENCE_SCHEMA } from "./manifest.mjs";

const REPO_ROOT = resolve(fileURLToPath(import.meta.url), "../../../..");
const sha = (value) => createHash("sha256").update(value).digest("hex");
const git = (args) => {
  const result = spawnSync("git", args, { cwd: REPO_ROOT, encoding: "utf8" });
  if (result.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout.trim();
};

function writeSealedAddon(dir, commit) {
  const components = [
    ["command", "membrane", true], ["service", "cortex-service", true], ["icon", "membrane-tab-icon.png", false],
    ["license", "LICENSE", false], ["eula", "EULA.txt", false], ["privacy", "PRIVACY.md", false], ["third-party-notices", "THIRD-PARTY-NOTICES.txt", false],
  ].map(([role, name, executable]) => {
    const content = `${role}:${name}`; writeFileSync(join(dir, name), content);
    return { role, name, sha256: sha(content), sizeBytes: Buffer.byteLength(content), executable, signing: executable ? { contract: "fixture", status: "verified" } : null };
  });
  const manifest = { schema: 1, kind: "headless-addon", addon: "membrane", version: "0.1.0", commit, platform: "mac", targetTriple: "aarch64-apple-darwin", consumer: { contract: "orthic-product-v1" }, files: components, signedAt: "2026-08-11T00:00:00.000Z" };
  const path = join(dir, "addon-manifest.json"); writeFileSync(path, `${JSON.stringify(manifest)}\n`);
  return path;
}

test("sealed add-on manifest rejects missing, wrong-schema, and tampered components", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-addon-manifest-"));
  try {
    assert.throws(() => readAddonManifest({ manifestPath: join(dir, "missing.json") }), /not found/);
    const path = writeSealedAddon(dir, git(["rev-parse", "HEAD"]));
    const parsed = readAddonManifest({ manifestPath: path });
    assert.equal(parsed.manifest.addon, "membrane");
    writeFileSync(join(dir, "membrane"), "tampered");
    assert.throws(() => readAddonManifest({ manifestPath: path }), /hash mismatch/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("buildProvenance and buildToolchain read declared state without compiling", () => {
  const commit = git(["rev-parse", "HEAD"]);
  assert.equal(buildProvenance({ repoRoot: REPO_ROOT, commit }).commit, commit);
  const toolchain = buildToolchain({ repoRoot: REPO_ROOT });
  assert.equal(toolchain.node.packageManager, "pnpm@11.18.0");
  assert.equal(toolchain.addon.packageManager, "pnpm@11.18.0");
  assert.equal(toolchain.rust.edition, "2021");
});

test("buildAddonEvidence binds sealed manifest and component hashes, then names only real remaining gaps", () => {
  const sealed = mkdtempSync(join(tmpdir(), "membrane-addon-sealed-"));
  const evidence = mkdtempSync(join(tmpdir(), "membrane-addon-evidence-"));
  try {
    const path = writeSealedAddon(sealed, git(["rev-parse", "HEAD"]));
    const report = buildAddonEvidence({ repoRoot: REPO_ROOT, manifestPath: path, evidenceRoot: evidence });
    assert.equal(report.artifact.sha256, sha(readFileSync(path)));
    assert.equal(report.addonManifest.files.length, 7);
    for (const file of [report.sbom, report.provenance, report.toolchain]) {
      assert.ok(file.path.startsWith(evidence)); assert.ok(existsSync(file.path)); assert.equal(sha(readFileSync(file.path)), file.sha256);
    }
    assert.ok(JSON.parse(readFileSync(report.sbom.path)).componentCount > 100);
    assert.deepEqual(report.gaps.map((gap) => gap.kind), ["tests", "signatures", "platform_trust", "install_receipts", "event_history"]);
  } finally { rmSync(sealed, { recursive: true, force: true }); rmSync(evidence, { recursive: true, force: true }); }
});

test("assembled release evidence still verifies when every required receipt exists", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-evidence-assemble-"));
  try {
    const file = (name, content = name) => { writeFileSync(join(dir, name), content); return { path: name, sha256: sha(content) }; };
    const commit = "b".repeat(40); const generation = "c".repeat(64); const artifact = file("addon-manifest.json");
    const trust = (kind) => ({ kind, identity: `${kind}-identity`, subject_sha256: artifact.sha256, receipt: file(`${kind}.receipt`) });
    const platformAcceptance = (os) => {
      const base = { schema: "orthic.membrane.platform-acceptance.v1", receiptId: `${os}-1`, mode: "clean-vm", commit, releaseGeneration: generation, version: "1.2.3", platform: os, artifact: { name: `${os}.artifact`, sha256: "d".repeat(64) }, trust: os === "macos" ? { codesign: "pass", notarization: "pass", staple: "pass", gatekeeper: "pass" } : { authenticode: "pass", publicTrust: "pass", rfc3161: "pass" }, lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" }, environment: { clean: true, machineDigest: "e".repeat(64), bypassWarnings: false } };
      return { os, vector_dispatch: "CORTEX_VECTOR_DISPATCH_V2", contract: file(`${os}.contract`, JSON.stringify({ schema: base.schema, commit: base.commit, releaseGeneration: base.releaseGeneration, version: base.version, platform: base.platform, artifact: base.artifact })), receipt: file(`${os}.receipt`, JSON.stringify(base)) };
    };
    const manifest = assembleReleaseEvidenceManifest({ release: { tag: "v1.2.3", commit, tree: "d".repeat(40), generation, target: "mac-arm64", artifact_sha256: artifact.sha256, vector_dispatch: "CORTEX_VECTOR_DISPATCH_V2" }, artifact, sbom: file("sbom"), provenance: file("provenance"), toolchain: file("toolchain"), tests: [file("test")], signatures: [trust("ed25519")], platform_trust: trust("apple-notary"), compatibility: file("compatibility"), install_receipts: [platformAcceptance("macos"), platformAcceptance("windows")], event_history: { status: "sealed", receipt: file("history") } });
    assert.equal(manifest.schema, RELEASE_EVIDENCE_SCHEMA);
    assert.equal(verifyReleaseEvidence(manifest, dir).verified, true);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
