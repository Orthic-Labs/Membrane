// D19: package-manager manifests — identities, immutable URLs, exact SHA-256
// placeholders, and MCP registry metadata.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const read = (p) => readFileSync(join(ROOT, p), "utf8");

test("Homebrew template uses immutable release URL and exact hash", () => {
  const formula = read("release/homebrew/cortex.rb.template");
  assert.ok(formula.includes("TEMPLATE_ONLY"));
  assert.ok(formula.includes("homepage"));
  assert.ok(formula.includes("releases/download/v__VERSION__"), "immutable versioned URL");
  assert.ok(formula.includes("cortex-__VERSION__.tgz"), "owner-controlled npm tarball");
  assert.ok(formula.includes("sha256 \"__NPM_TARBALL_SHA256__\""), "exact hash placeholder");
  assert.ok(!formula.includes("latest"), "never points at latest");
});

test("WinGet manifests carry the stable package ID", () => {
  for (const file of ["release/winget/OrthicLabs.Cortex.version.template.json", "release/winget/OrthicLabs.Cortex.installer.template.json", "release/winget/OrthicLabs.Cortex.locale.template.json"]) {
    const manifest = JSON.parse(read(file));
    assert.equal(manifest.PackageIdentifier, "OrthicLabs.Cortex");
  }
});

test("Scoop manifest has 64bit URL and hash", () => {
  const scoop = JSON.parse(read("release/scoop/cortex.json.template"));
  assert.equal(scoop._template, true);
  assert.ok(scoop.architecture["64bit"].url.endsWith("cortex-__VERSION__.tgz"));
  assert.equal(scoop.architecture["64bit"].hash, "__NPM_TARBALL_SHA256__");
  assert.deepEqual(scoop.bin, ["cortex.cmd", "cortex-mcp.cmd"]);
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
  assert.ok(existsSync(join(ROOT, "build/Dockerfile")));
  assert.ok(existsSync(join(ROOT, "build/.dockerignore")));
  assert.ok(existsSync(join(ROOT, "release", "container", "README.md")));
});

test("all manifests reference versioned identities, not latest", () => {
  const server = read("server.json");
  assert.ok(!server.includes("latest"));
});

test("tracked release tree contains templates, never final instances", () => {
  for (const path of ["release/catalog.json", "release/compatibility.json", "release/homebrew/cortex.rb", "release/scoop/cortex.json", "release/winget/OrthicLabs.Cortex.version.json", "release/winget/OrthicLabs.Cortex.installer.json", "release/winget/OrthicLabs.Cortex.locale.json"]) {
    assert.equal(existsSync(join(ROOT, path)), false, path);
  }
  assert.equal(JSON.parse(read("release/catalog.template.json")).publishable, false);
  assert.equal(JSON.parse(read("release/compatibility.template.json")).publishable, false);
});
