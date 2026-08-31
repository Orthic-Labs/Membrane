import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { assertCandidateSourceClean } from "./release/candidate-source-clean.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (relativePath) => readFileSync(join(root, relativePath), "utf8");

test("generated CI remains right-git managed & reaches repository gate", () => {
  const workflow = read(".github/workflows/ci.yml");
  const rightGit = JSON.parse(read(".rightgit.json"));

  assert.match(workflow, /^# Managed by right-git — do not hand-edit\./m);
  assert.match(workflow, /^# Template: profiles\/rust-hybrid\/ci\.yml \| Template version: 1\.4\.0$/m);
  assert.match(workflow, /run: bash \.\/scripts\/ci\/run-ci\.sh/);
  assert.equal(rightGit.profile, "rust-hybrid");
  assert.equal(rightGit.gate.path, "scripts/ci/run-ci.sh");
  assert.doesNotMatch(workflow, /run:\s*(?:bash\s+)?cargo\b/);
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
	assert.match(chain, /gh", \["release", "upload", `v\$\{version\}`, installer, "--clobber"\]/);
	assert.match(chain, /notarized: true, stapled: true/);
	assert.match(chain, /macOS finalization receipt does not bind the exact notarized & stapled DMG/);
	assert.match(chain, /gh", \["release", "upload", `v\$\{version\}`, dmg, finalizationPath, "--clobber"\]/);
	assert.match(chain, /nsis-embedded-receipt\.json/);
	assert.match(chain, /verifyNsisEmbeddedBinary/);
	assert.match(chain, /size:\s*installerSize/);
	assert.match(chain, /version !== "0\.1\.18"/);
	assert.match(chain, /requires one prior stable-layout signed Windows installer/);
});

test("each protected finalizer materializes Hub dependencies from Hub lockfile", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /function finalizeWindows\(\)[\s\S]*pnpm\.cmd", \["--dir", hub, "install", "--frozen-lockfile"\]/);
	assert.match(chain, /function finalizeMac\(\)[\s\S]*pnpm", \["--dir", hub, "install", "--frozen-lockfile"\]/);
});
