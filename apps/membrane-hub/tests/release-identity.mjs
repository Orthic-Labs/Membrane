import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { engineReleaseIdentity } from "../scripts/release-identity.mjs";
const hub = new URL("..", import.meta.url).pathname;
const repo = new URL("../../..", import.meta.url).pathname.replace(/\/$/, "");
const writer = join(hub, "scripts/write-release-manifest.mjs");
const distIdentity = join(hub, "dist/release-identity.json");
const originalDist = existsSync(distIdentity) ? readFileSync(distIdentity) : null;
function manifest(identity) {
  return { keep: { asset: "sealed" }, crypt_source_commit: "old", source_tree_path: "old", source_tree_sha256: "old", release_generation: "old", assets: [{ sha256: "unchanged" }] };
}
function run(args) {
  const result = execFileSync(process.execPath, [writer, ...args], { cwd: hub, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  return result;
}
function rejected(args) {
  assert.throws(() => execFileSync(process.execPath, [writer, ...args], { cwd: hub, stdio: "pipe" }));
}
test.after(() => {
  if (originalDist) writeFileSync(distIdentity, originalDist);
  else rmSync(distIdentity, { force: true });
});
test("default ignores stale dist identity & uses current HEAD", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-release-"));
  const path = join(dir, "manifest.json");
  const identity = engineReleaseIdentity(repo);
  mkdirSync(join(hub, "dist"), { recursive: true });
  writeFileSync(distIdentity, JSON.stringify({ ...identity, commit: "0".repeat(40) }));
  writeFileSync(path, JSON.stringify(manifest(identity)));
  run(["--manifest", path]);
  const output = JSON.parse(readFileSync(path, "utf8"));
  assert.deepEqual(Object.fromEntries(Object.entries(output).filter(([key]) => key !== "keep" && key !== "assets")), {
    crypt_source_commit: identity.commit, source_tree_path: "engine", source_tree_sha256: identity.sourceTreeSha256, release_generation: identity.releaseGeneration,
  });
  assert.deepEqual(output.assets, [{ sha256: "unchanged" }]);
  rmSync(dir, { recursive: true, force: true });
});
test("stale or malformed explicit identity rejects without writing", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-release-"));
  const path = join(dir, "manifest.json");
  const identityPath = join(dir, "identity.json");
  const identity = engineReleaseIdentity(repo);
  const original = JSON.stringify(manifest(identity));
  writeFileSync(path, original);
  writeFileSync(identityPath, JSON.stringify({ ...identity, commit: "0".repeat(40) }));
  rejected(["--manifest", path, "--identity", identityPath]);
  writeFileSync(identityPath, JSON.stringify({ commit: identity.commit }));
  rejected(["--manifest", path, "--identity", identityPath]);
  assert.equal(readFileSync(path, "utf8"), original);
  rmSync(dir, { recursive: true, force: true });
});
test("fresh recorded identity updates exactly four manifest fields & check validates all", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-release-"));
  const path = join(dir, "manifest.json");
  const identityPath = join(dir, "identity.json");
  const identity = engineReleaseIdentity(repo);
  const before = manifest(identity);
  writeFileSync(path, JSON.stringify(before));
  writeFileSync(identityPath, JSON.stringify(identity));
  run(["--manifest", path, "--identity", identityPath]);
  const after = JSON.parse(readFileSync(path, "utf8"));
  assert.deepEqual(after.keep, before.keep); assert.deepEqual(after.assets, before.assets);
  assert.equal(Object.keys(after).filter((key) => JSON.stringify(after[key]) !== JSON.stringify(before[key])).length, 4);
  run(["--manifest", path, "--identity", identityPath, "--check"]);
  after.release_generation = "sha256:bad"; writeFileSync(path, JSON.stringify(after));
  rejected(["--manifest", path, "--identity", identityPath, "--check"]);
  rmSync(dir, { recursive: true, force: true });
});
test("package scripts build, record identity, then return to RightKit", () => {
  const pkg = JSON.parse(readFileSync(join(hub, "package.json"), "utf8"));
  assert.equal(pkg.scripts["rightkit:package:mac"], "node scripts/build-mac-release.mjs");
  assert.equal(pkg.scripts["rightkit:package:win"], "node scripts/build-windows-release.mjs");
  for (const name of ["build-mac-release.mjs", "build-windows-release.mjs"]) {
    const source = readFileSync(join(hub, "scripts", name), "utf8");
    assert.ok(source.indexOf('tauri", "build') < source.indexOf('write-release-manifest.mjs'));
  }
});
