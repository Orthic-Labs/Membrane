// MBR-906: proves the per-platform optionalDependency package scaffolds
// under npm/platforms/** stay in lockstep with npm/index.mjs's
// PLATFORM_PACKAGES registry (the source of truth the bootstrapper's
// selectPlatform() actually resolves against), carry valid npm os/cpu
// fields, and ship no native binary (this task never bundles the core app).
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";
import { PLATFORM_PACKAGES } from "../../npm/index.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const PLATFORMS_ROOT = resolve(HERE, "../../npm/platforms");
const ROOT_PACKAGE = JSON.parse(readFileSync(resolve(HERE, "../../npm/package.json"), "utf8"));

const EXPECTED_OS = { darwin: "darwin", linux: "linux", win32: "win32" };

function platformKeyParts(key) {
  const [os, cpu] = key.split(/-(?=[^-]+$)/);
  return { os, cpu };
}

test("every PLATFORM_PACKAGES entry has a matching npm/platforms/<key>/package.json", () => {
  const dirs = readdirSync(PLATFORMS_ROOT).sort();
  assert.deepEqual(dirs, Object.keys(PLATFORM_PACKAGES).sort());
});

test("each per-platform package.json declares the exact registered name and valid os/cpu constraints", () => {
  for (const [key, expectedName] of Object.entries(PLATFORM_PACKAGES)) {
    const pkg = JSON.parse(readFileSync(resolve(PLATFORMS_ROOT, key, "package.json"), "utf8"));
    assert.equal(pkg.name, expectedName, `${key} package.json name must match npm/index.mjs's PLATFORM_PACKAGES`);
    const { os, cpu } = platformKeyParts(key);
    assert.deepEqual(pkg.os, [EXPECTED_OS[os]], `${key} os field`);
    assert.deepEqual(pkg.cpu, [cpu], `${key} cpu field`);
    assert.match(pkg.version, /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/, `${key} version must be valid semver`);
  }
});

test("no per-platform package declares a bundled native binary (this task resolves and fetches; it does not ship binaries)", () => {
  for (const key of Object.keys(PLATFORM_PACKAGES)) {
    const pkg = JSON.parse(readFileSync(resolve(PLATFORMS_ROOT, key, "package.json"), "utf8"));
    assert.equal(pkg.main, undefined, `${key} must not declare a bundled entry binary`);
    assert.equal(pkg.files, undefined, `${key} must not declare bundled files yet (no real artifact exists)`);
  }
});

test("the root @orthic/membrane package depends on every platform package as an optionalDependency", () => {
  for (const name of Object.values(PLATFORM_PACKAGES)) {
    assert.ok(name in (ROOT_PACKAGE.optionalDependencies ?? {}), `root package.json must list ${name} as an optionalDependency`);
  }
  // The root package itself never bundles the platform packages' code:
  // "files" ships only the bootstrapper, not per-platform manifests.
  assert.deepEqual(ROOT_PACKAGE.files, ["index.mjs", "bin/membrane.mjs", "package.json"]);
});
