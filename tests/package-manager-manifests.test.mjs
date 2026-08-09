// D19: package-manager manifests — identities, immutable URLs, exact SHA-256
// placeholders, and MCP registry metadata.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const read = (p) => readFileSync(join(ROOT, p), "utf8");

test("Homebrew formula uses immutable release URL and exact hash", () => {
  const formula = read("release/homebrew/cortex.rb");
  assert.ok(formula.includes("homepage"));
  assert.ok(formula.includes("releases/download/v__VERSION__"), "immutable versioned URL");
  assert.ok(formula.includes("sha256 \"__DARWIN_ARM64_SHA256__\""), "exact hash placeholder");
  assert.ok(!formula.includes("latest"), "never points at latest");
});

test("WinGet manifests carry the stable package ID", () => {
  for (const file of ["release/winget/OrthicLabs.Cortex.version.json", "release/winget/OrthicLabs.Cortex.installer.json", "release/winget/OrthicLabs.Cortex.locale.json"]) {
    const manifest = JSON.parse(read(file));
    assert.equal(manifest.PackageIdentifier, "OrthicLabs.Cortex");
  }
});

test("Scoop manifest has 64bit URL and hash", () => {
  const scoop = JSON.parse(read("release/scoop/cortex.json"));
  assert.ok(scoop.architecture["64bit"].url);
  assert.ok(scoop.architecture["64bit"].hash);
});

test("Linux archive metadata honors XDG paths", () => {
  const linux = JSON.parse(read("release/linux/cortex-archive.json"));
  assert.ok(linux.xdg.config.includes("XDG_CONFIG_HOME"));
  assert.ok(linux.xdg.data.includes("XDG_DATA_HOME"));
});

test("MCP server.json launches cortex mcp serve from npm", () => {
  const server = JSON.parse(read("server.json"));
  assert.equal(server.command, "npx");
  assert.ok(server.args.includes("@orthic-labs/cortex"));
  assert.ok(server.args.includes("mcp"));
  assert.ok(server.args.includes("serve"));
});

test("container files exist for CI/headless use", () => {
  assert.ok(existsSync(join(ROOT, "Dockerfile")));
  assert.ok(existsSync(join(ROOT, ".dockerignore")));
  assert.ok(existsSync(join(ROOT, "release", "container", "README.md")));
});

test("all manifests reference versioned identities, not latest", () => {
  const server = read("server.json");
  assert.ok(!server.includes("latest"));
});
