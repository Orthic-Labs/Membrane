import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { assertCandidateSourceClean } from "./release/candidate-source-clean.mjs";
import { REQUIRED_RELEASE_STAGES, assertSourceRevisionIsAncestorOfMain, runWithDraftCleanupOnFailure, selectPriorInstallerRelease } from "./release/right-git-release-chain.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relativePath) => readFileSync(join(root, relativePath), "utf8");

test("generated CI remains right-git managed & reaches repository gate", () => {
  const workflow = read(".github/workflows/ci.yml");
  const rightGit = JSON.parse(read(".rightgit.json"));
  const gate = read("scripts/ci/run-ci.sh");

  assert.match(workflow, /^# Managed by right-git — do not hand-edit\./m);
  assert.ok(workflow.includes(`# Template: profiles/rust-hybrid/ci.yml | Template version: ${rightGit.templateVersion}`));
  assert.match(workflow, /run: bash \.\/scripts\/ci\/run-ci\.sh/);
  assert.equal(rightGit.compile, "github-actions-only");
  assert.equal(rightGit.profile, "rust-hybrid");
  assert.equal(rightGit.gate.path, "scripts/ci/run-ci.sh");
  assert.equal(rightGit.rustCache.requiredForGate, true);
  assert.doesNotMatch(workflow, /run:\s*(?:bash\s+)?cargo\b/);
  assert.match(gate, /RIGHT_GIT_RUST_CHANGED:-true/);
  assert.match(gate, /--package membrane --bin membrane/);
  assert.match(gate, /--package membrane-runtime --example hub_runtime_test_host/);
});

test("equivalence Cargo stages enter through RightKit", () => {
  const equivalence = read("scripts/ci/run-equivalence.mjs");
  const packageJson = JSON.parse(read("package.json"));

  assert.match(equivalence, /const rightkit = process\.env\.RIGHTKIT/);
  assert.match(equivalence, /run\(rightkit, \[\s*"cargo",\s*"build"/);
  assert.match(equivalence, /run\(rightkit, \[\s*"cargo",\s*"test"/);
  assert.doesNotMatch(equivalence, /run\("cargo",/);
  for (const [name, command] of Object.entries(packageJson.scripts)) {
    if (/\bcargo\b/.test(command)) assert.match(command, /\brightkit(?:\.cmd)?\s+cargo\b/, name);
  }
});

test("Windows release artifacts derive from canonical Cargo target resolution", () => {
  const releaseConfig = read("apps/membrane-hub/right-release.config.mjs");

  assert.match(releaseConfig, /from "@rightkit\/release\/cargo-target\.mjs"/);
  assert.match(releaseConfig, /resolveTargetRoot\(join\(hubRoot, "src-tauri", "Cargo\.toml"\)\)/);
  assert.match(releaseConfig, /const releaseRoot = join\(cargoTargetRoot, cargoTriple, "release"\)/);
  assert.match(releaseConfig, /join\(releaseRoot, "bundle", "nsis"/);
  assert.match(releaseConfig, /join\(releaseRoot, "membrane-hub\.exe"\)/);
  assert.doesNotMatch(releaseConfig, /const win(?:Installer|RawExe)\s*=\s*["'`][^"'`]*src-tauri[\\/]target[\\/]/);
});

test("each release candidate materializes Hub dependencies from Hub lockfile", () => {
	const candidate = read("scripts/release/right-git-candidate.mjs");
	const windows = read("apps/membrane-hub/scripts/release-build-candidate-windows.mjs");

	assert.match(candidate, /platform === "windows"[\s\S]*pnpm\.cmd", \["--dir", hub, "install", "--frozen-lockfile"\]/);
	assert.match(candidate, /platform === "macos"[\s\S]*pnpm", \["--dir", hub, "install", "--frozen-lockfile"\]/);
	assert.match(candidate, /MEMBRANE_PUBLIC_CI_DIRECT_CARGO: "1"/);
	assert.match(candidate, /allowGeneratedSchemaOutput: mode === "check"/);
	assert.match(candidate, /assertCandidateSourceClean\(\{ git, allowGeneratedSchemaOutput \}\)/);
	assert.match(windows, /cargo", \["build", "--locked"/);
	assert.match(windows, /cargo", \["metadata", "--locked"/);
});

test("macOS release is signed & notarized through RightRelease before stapling", () => {
  const releaseConfig = read("apps/membrane-hub/right-release.config.mjs");
  const packageJson = JSON.parse(read("apps/membrane-hub/package.json"));
  const buildMac = read("apps/membrane-hub/scripts/build-mac-release.mjs");
  const finalizeMac = read("apps/membrane-hub/scripts/release-build-mac.mjs");
  const releaseChain = read("scripts/release/right-git-release-chain.mjs");

  assert.match(releaseConfig, /mac:\s*\{[\s\S]*signingContract: "macos-developer-id-notarized-portable-v1"/);
  assert.match(releaseConfig, /prePackage: \{ cmd: "pnpm", args: \["run", "rightkit:prepackage:mac"\] \}/);
  assert.match(releaseConfig, /sign: \{ prePackageFiles: \[macDmg\] \}/);
  assert.match(releaseConfig, /notarize: \{ file: macDmg \}/);
  assert.equal(packageJson.scripts["rightkit:prepackage:mac"], "node scripts/build-mac-release.mjs prepare");
  assert.equal(packageJson.scripts["rightkit:package:mac"], "node scripts/build-mac-release.mjs package");
  assert.doesNotMatch(buildMac, /notarytool|stapler|spctl/);
  assert.match(finalizeMac, /\["stapler", "staple", dmg\]/);
  assert.match(finalizeMac, /\["stapler", "validate", dmg\]/);
  assert.match(finalizeMac, /"spctl"/);
  assert.match(releaseChain, /release:build:mac"\], repo, \{ \.\.\.process\.env, MEMBRANE_PUBLIC_CI_DIRECT_CARGO: "1" \}/);
});

test("candidate source check accepts Tauri's same-byte manifest rewrite & rejects real source drift", () => {
	const repo = mkdtempSync(join(tmpdir(), "membrane-candidate-source-"));
	const cargoToml = join(repo, "apps", "membrane-hub", "src-tauri", "Cargo.toml");
	const git = (args) => execFileSync("git", args, { cwd: repo, encoding: "utf8" });
	try {
		mkdirSync(dirname(cargoToml), { recursive: true });
		writeFileSync(cargoToml, '[package]\nname = "membrane-hub"\n');
		git(["init", "-q"]);
		git(["config", "user.email", "release@example.invalid"]);
		git(["config", "user.name", "Membrane Release"]);
		git(["add", "."]);
		git(["commit", "-qm", "fixture"]);

		// This mirrors Tauri's Windows rewrite: a new write with identical bytes.
		writeFileSync(cargoToml, readFileSync(cargoToml));
		assert.doesNotThrow(() => assertCandidateSourceClean({ git }));

		writeFileSync(cargoToml, '[package]\nname = "tampered"\n');
		assert.throws(() => assertCandidateSourceClean({ git }), /Cargo\.toml/);
		git(["checkout", "--", "."]);

		const generated = join(repo, "apps", "membrane-hub", "src-tauri", "gen", "schema.json");
		mkdirSync(dirname(generated), { recursive: true });
		writeFileSync(generated, "{}");
		assert.throws(() => assertCandidateSourceClean({ git }), /schema\.json/);
		assert.doesNotThrow(() => assertCandidateSourceClean({ git, allowGeneratedSchemaOutput: true }));
	} finally {
		rmSync(repo, { recursive: true, force: true });
	}
});

test("qualification uses hosted-gh-compatible prior download & publication uploads both installers", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /gh", \["release", "download", prior\.tag_name/);
	assert.doesNotMatch(chain, /"gh", \["api"[^\n]*"--output"/);
	assert.match(chain, /gh", \["release", "upload", tag, installer, "--clobber"\]/);
	assert.match(chain, /notarized: true, stapled: true/);
	assert.match(chain, /macOS finalization receipt does not bind the exact notarized & stapled DMG/);
	assert.match(chain, /gh", \["release", "upload", tag, dmg, finalizationPath, "--clobber"\]/);
	assert.match(chain, /nsis-embedded-receipt\.json/);
	assert.match(chain, /verifyNsisEmbeddedBinary/);
	assert.match(chain, /size:\s*installerSize/);
	assert.doesNotMatch(chain, /version !== "0\.1\.18"/);
	assert.match(chain, /releases\.find\(\(release\)[^\n]*windowsInstallerAsset\(release\)\)/);
	assert.match(chain, /if \(prior && asset\)/);
	assert.match(chain, /args\.push\("-PreviousInstaller", previous\)/);
});

test("publication is draft-first: create draft, upload signed artifacts unchanged, re-verify uploaded hashes, then publish", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /gh", \["release", "create", tag, "--target", sourceRevision, "--title", tag, "--draft"/);
	assert.match(chain, /reVerifyUploadedAssetHash\(tag, installer\)/);
	assert.match(chain, /reVerifyUploadedAssetHash\(tag, dmg\)/);
	assert.match(chain, /gh", \["release", "edit", tag, "--draft=false"\]/);
	assert.match(chain, /function reVerifyUploadedAssetHash\(tag, sourcePath\)/);
	assert.match(chain, /requireVerifiedEvidence\(\)/);
	assert.match(chain, /runWithDraftCleanupOnFailure\(tag, \(\) => \{/);
});

test("publication cleanup-on-failure: deletes the draft & rethrows the original error without letting cleanup failures mask it", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	// Source-level proof the cleanup primitive never deletes an already-published release.
	assert.match(chain, /export function deleteDraftReleaseIfStillDraft\(tag\)/);
	assert.match(chain, /if \(!isDraft\) return;/);
	assert.match(chain, /"release", "delete", tag, "--yes"/);

	// Behavioral proof of the wrap/rethrow/no-mask contract, with gh & the network
	// fully mocked out — this is the same failure mode that burned v0.1.18-v0.1.23:
	// a transient failure must not leave an orphan draft that blocks retrying the
	// same version, and a broken cleanup must never hide the real failure.
	const publicationCalls = [];
	const failingAction = () => {
		publicationCalls.push("upload");
		throw new Error("upload failed: network reset");
	};
	const cleanupCalls = [];
	assert.throws(
		() => runWithDraftCleanupOnFailure("v9.9.9", failingAction, (tag) => cleanupCalls.push(tag)),
		/upload failed: network reset/,
	);
	assert.deepEqual(publicationCalls, ["upload"]);
	assert.deepEqual(cleanupCalls, ["v9.9.9"]);

	// A cleanup failure must never mask the original error.
	assert.throws(
		() => runWithDraftCleanupOnFailure("v9.9.9", failingAction, () => { throw new Error("gh release delete failed"); }),
		/upload failed: network reset/,
	);

	// Success path: cleanup must never run when publication actually succeeds.
	let cleanupRanOnSuccess = false;
	runWithDraftCleanupOnFailure("v9.9.9", () => {}, () => { cleanupRanOnSuccess = true; });
	assert.equal(cleanupRanOnSuccess, false);
});

test("admission validates the SHA-bound dispatch envelope & emits admitted facts to GITHUB_OUTPUT", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /ref !== "refs\/heads\/main"/);
	assert.match(chain, /Number\(runAttempt\) !== 1/);
	assert.match(chain, /\^\\d\+\\\.\\d\+\\\.\\d\+\$/);
	assert.match(chain, /\^\[a-f0-9\]\{40\}\$/);
	assert.match(chain, /assertSourceRevisionIsAncestorOfMain\(revision\)/);
	assert.match(chain, /merge-base", "--is-ancestor", revision, mainRef/);
	assert.doesNotMatch(chain, /merge-base", "--is-ancestor", revision, "HEAD"/);
	assert.match(chain, /publish && !signedQualification/);
	assert.match(chain, /tagOrReleaseExists\(releaseVersion\)/);
	assert.match(chain, /appendFileSync\(outputPath, `\$\{key\}=\$\{value\}\\n`\)/);
});

test("stage-summary records init/finalize identity & evidence; evidence-verification requires every stage to have SUCCEEDED", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /RIGHT_GIT_STAGE_ACTION/);
	assert.match(chain, /stage-summary\.json/);
	assert.match(chain, /status === "SUCCEEDED" && exitCode !== 0/);
	assert.match(chain, /REQUIRED_RELEASE_STAGES/);
	assert.match(chain, /evidence-verification\.json/);
	assert.match(chain, /manifest\.signing\?\.status !== "signed" \|\| manifest\.artifact\?\.sha256 !== installerSha256/);
	assert.match(chain, /finalization\.notarized !== true \|\| finalization\.stapled !== true \|\| finalization\.artifact\?\.sha256 !== dmgSha256/);
});

test("required release stages match the RightKit generator's RIGHT_GIT_STAGE values & aggregate under RIGHT_GIT_STAGE_SUMMARY_ROOT", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /RIGHT_GIT_STAGE_SUMMARY_ROOT/);
	assert.doesNotMatch(chain, /RIGHT_GIT_EVIDENCE_ROOT/);
	assert.match(chain, /\{ stage: "candidate", platform: "windows", architecture: "x86_64" \}/);
	assert.match(chain, /\{ stage: "candidate", platform: "macos", architecture: "arm64" \}/);
	assert.match(chain, /\{ stage: "windows-sign", platform: "windows", architecture: "x86_64" \}/);
	assert.match(chain, /\{ stage: "macos-sign", platform: "macos", architecture: "arm64" \}/);
	assert.match(chain, /\{ stage: "installed-qualification" \}/);
	assert.doesNotMatch(chain, /stage: "finalize-windows"/);
	assert.doesNotMatch(chain, /stage: "finalize-macos"/);
	assert.doesNotMatch(chain, /stage: "qualify-installed"/);
	// evidence-verification must bind to the exact run: a stage summary from a
	// different run id/attempt (a stale artifact) cannot satisfy the requirement.
	assert.match(chain, /summary\.runId === runId && summary\.runAttempt === runAttempt/);
});

test("admission ancestor check genuinely rejects a source revision that is not reachable from remote main", () => {
	const upstream = mkdtempSync(join(tmpdir(), "membrane-admission-upstream-"));
	const checkout = mkdtempSync(join(tmpdir(), "membrane-admission-checkout-"));
	const git = (dir, args) => execFileSync("git", args, { cwd: dir, encoding: "utf8" }).trim();
	try {
		git(upstream, ["init", "-q", "--initial-branch", "main"]);
		git(upstream, ["config", "user.email", "release@example.invalid"]);
		git(upstream, ["config", "user.name", "Membrane Release"]);
		writeFileSync(join(upstream, "a.txt"), "a\n");
		git(upstream, ["add", "."]);
		git(upstream, ["commit", "-qm", "a"]);
		const ancestorSha = git(upstream, ["rev-parse", "HEAD"]);
		writeFileSync(join(upstream, "b.txt"), "b\n");
		git(upstream, ["add", "."]);
		git(upstream, ["commit", "-qm", "b"]);

		git(checkout, ["init", "-q", "--initial-branch", "work"]);
		git(checkout, ["config", "user.email", "release@example.invalid"]);
		git(checkout, ["config", "user.name", "Membrane Release"]);
		git(checkout, ["remote", "add", "origin", upstream]);

		// origin/main is not yet present locally: the fallback fetch path must resolve it,
		// and a genuine ancestor of the real remote main must be accepted.
		assert.doesNotThrow(() => assertSourceRevisionIsAncestorOfMain(ancestorSha, { cwd: checkout }));

		// A commit with no relation to upstream main must be genuinely rejected — this is
		// exactly the regression a HEAD-vs-HEAD tautological comparison would miss, since
		// admission checks out `ref: source_revision`, making HEAD equal the SHA under test.
		writeFileSync(join(checkout, "stray.txt"), "stray\n");
		git(checkout, ["add", "."]);
		git(checkout, ["commit", "-qm", "stray"]);
		const strayCommit = git(checkout, ["rev-parse", "HEAD"]);
		assert.throws(() => assertSourceRevisionIsAncestorOfMain(strayCommit, { cwd: checkout }), /not an ancestor of main/);
	} finally {
		rmSync(upstream, { recursive: true, force: true });
		rmSync(checkout, { recursive: true, force: true });
	}
});

test("prior-installer selection: no prior installer has ever been published selects first-release same-version repair", () => {
	assert.equal(selectPriorInstallerRelease([], "0.1.24"), null);
	const noInstallers = [
		{ tag_name: "v0.1.20", draft: false, prerelease: false, assets: [] },
		{ tag_name: "v0.1.19", draft: false, prerelease: false, assets: [{ name: "sbom.json" }] },
	];
	assert.equal(selectPriorInstallerRelease(noInstallers, "0.1.24"), null);
});

test("prior-installer selection: newest tag lacks an installer but an older release has one selects the older release", () => {
	const older = { tag_name: "v0.1.20", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.20_x64-setup.exe" }] };
	const releases = [
		{ tag_name: "v0.1.23", draft: false, prerelease: false, assets: [{ name: "sbom.json" }] },
		{ tag_name: "v0.1.21", draft: false, prerelease: false, assets: [] },
		older,
	];
	assert.equal(selectPriorInstallerRelease(releases, "0.1.24"), older);
});

test("prior-installer selection: a prior installer exists selects prior-version upgrade/repair qualification against it", () => {
	const newest = { tag_name: "v0.1.23", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.23_x64-setup.exe" }] };
	const older = { tag_name: "v0.1.20", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.20_x64-setup.exe" }] };
	const releases = [newest, older];
	assert.equal(selectPriorInstallerRelease(releases, "0.1.24"), newest);
	// Draft/prerelease/self-version/pre-stable-layout entries must never be selected, even with an installer asset.
	const noise = [
		{ tag_name: "v0.1.24", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.24_x64-setup.exe" }] },
		{ tag_name: "v0.1.22", draft: true, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.22_x64-setup.exe" }] },
		{ tag_name: "v0.1.21", draft: false, prerelease: true, assets: [{ name: "Membrane_Hub_0.1.21_x64-setup.exe" }] },
		{ tag_name: "v0.1.17", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.17_x64-setup.exe" }] },
		older,
	];
	assert.equal(selectPriorInstallerRelease(noise, "0.1.24"), older);
});

test("prior-installer selection: eligibility is a real numeric semver comparison, not a version-pinned regex — future 0.2.x & 1.x releases are still selected", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");
	assert.doesNotMatch(chain, /\^v0\\\.1\\\./); // the old version-pinned regex must not come back
	assert.match(chain, /STABLE_INSTALLER_LAYOUT_FLOOR = \{ major: 0, minor: 1, patch: 18 \}/);
	assert.match(chain, /compareSemver\(parsed, STABLE_INSTALLER_LAYOUT_FLOOR\) >= 0/);

	// A future minor release (0.2.x) with an installer must still be selected: the
	// old regex only ever matched v0.1.x and would silently stop working here.
	const v020 = { tag_name: "v0.2.0", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.2.0_x64-setup.exe" }] };
	assert.equal(selectPriorInstallerRelease([v020], "0.2.1"), v020);

	// A future major release (1.x) must also be selected.
	const v100 = { tag_name: "v1.0.0", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_1.0.0_x64-setup.exe" }] };
	assert.equal(selectPriorInstallerRelease([v100], "1.0.1"), v100);

	// A pre-stable-layout release stays excluded under real numeric comparison too
	// (0.1.17 < 0.1.18 numerically, not just lexically).
	const tooOld = { tag_name: "v0.1.17", draft: false, prerelease: false, assets: [{ name: "Membrane_Hub_0.1.17_x64-setup.exe" }] };
	assert.equal(selectPriorInstallerRelease([tooOld, v020], "0.2.1"), v020);

	// A malformed tag must never match (parseSemverTag returns null for it).
	const malformed = { tag_name: "not-a-version", draft: false, prerelease: false, assets: [{ name: "setup.exe" }] };
	assert.equal(selectPriorInstallerRelease([malformed], "0.2.1"), null);
});

test("each protected finalizer materializes Hub dependencies from Hub lockfile", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /function finalizeWindows\(\)[\s\S]*pnpm\.cmd", \["--dir", hub, "install", "--frozen-lockfile"\]/);
	assert.match(chain, /function finalizeMac\(\)[\s\S]*pnpm", \["--dir", hub, "install", "--frozen-lockfile"\]/);
});

// The render-side tests in @rightkit/git prove the workflow is internally
// consistent; they cannot see what THIS repo's evidence verifier demands of it.
// This is the seam that broke: the generator emits no
// RIGHT_GIT_RELEASE_PLATFORM/ARCHITECTURE for the non-matrixed
// installed-qualification stage, so a required entry keyed on platform there is
// permanently unsatisfiable and publication fails after every expensive stage
// has already passed. Assert the two sides agree, stage by stage.
test("every REQUIRED_RELEASE_STAGES entry is satisfiable by the generated release-candidate workflow", () => {
  const workflow = read(".github/workflows/release-candidate.yml");
  // One block per stage-summary step: the env keys between `RIGHT_GIT_STAGE:`
  // and the end of that step's env mapping.
  const blocks = workflow
    .split(/^ {6}- name: /m)
    .filter((step) => / {10}RIGHT_GIT_STAGE: /.test(step))
    .map((step) => ({
      stage: /^ {10}RIGHT_GIT_STAGE: (\S+)$/m.exec(step)?.[1],
      platform: /^ {10}RIGHT_GIT_RELEASE_PLATFORM: (.+)$/m.exec(step)?.[1],
      architecture: /^ {10}RIGHT_GIT_RELEASE_ARCHITECTURE: (.+)$/m.exec(step)?.[1],
    }));
  assert.ok(blocks.length > 0, "no stage-summary steps found in the generated workflow");

  for (const required of REQUIRED_RELEASE_STAGES) {
    const emitted = blocks.filter((block) => block.stage === required.stage);
    assert.ok(emitted.length > 0, `generated workflow never emits stage ${required.stage}`);
    for (const block of emitted) {
      if (required.platform === undefined) {
        // Non-matrixed stage: the verifier must not key on a fact the
        // generator does not record, or the stage can never be satisfied.
        assert.equal(block.platform, undefined, `stage ${required.stage} is required without a platform but the workflow emits one`);
        assert.equal(block.architecture, undefined, `stage ${required.stage} is required without an architecture but the workflow emits one`);
      } else {
        // Matrixed stage: the generator must record platform & architecture,
        // otherwise the summary carries null and never matches.
        assert.ok(block.platform, `stage ${required.stage} is required with platform ${required.platform} but the workflow emits no RIGHT_GIT_RELEASE_PLATFORM`);
        assert.ok(block.architecture, `stage ${required.stage} is required with architecture ${required.architecture} but the workflow emits no RIGHT_GIT_RELEASE_ARCHITECTURE`);
      }
    }
  }
});
