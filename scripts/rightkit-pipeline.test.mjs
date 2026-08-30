import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

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
	assert.match(candidate, /apps\/membrane-hub\/src-tauri\/gen\//);
	assert.match(windows, /cargo", \["build", "--locked"/);
	assert.match(windows, /cargo", \["metadata", "--locked"/);
});

test("qualified publication uploads exact notarized macOS DMG beside Windows release assets", () => {
	const chain = read("scripts/release/right-git-release-chain.mjs");

	assert.match(chain, /notarized: true, stapled: true/);
	assert.match(chain, /macOS finalization receipt does not bind the exact notarized & stapled DMG/);
	assert.match(chain, /gh", \["release", "upload", `v\$\{version\}`, dmg, finalizationPath, "--clobber"\]/);
});
