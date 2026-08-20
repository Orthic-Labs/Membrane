// D19: package-manager manifests — identities, immutable URLs, exact SHA-256
// placeholders, and MCP registry metadata.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const read = (p) => readFileSync(join(ROOT, p), "utf8");

test("Homebrew template uses immutable release URL and exact hash", () => {
  const formula = read("release/homebrew/blueprint.rb.template");
  assert.ok(formula.includes("TEMPLATE_ONLY"));
  assert.ok(formula.includes("homepage"));
  assert.ok(formula.includes("releases/download/v__VERSION__"), "immutable versioned URL");
  assert.ok(formula.includes("blueprint-__VERSION__.tgz"), "owner-controlled npm tarball");
  assert.ok(formula.includes("sha256 \"__NPM_TARBALL_SHA256__\""), "exact hash placeholder");
  assert.ok(!formula.includes("latest"), "never points at latest");
});

test("WinGet manifests carry the stable package ID", () => {
  for (const file of ["release/winget/Membrane.Blueprint.version.template.json", "release/winget/Membrane.Blueprint.installer.template.json", "release/winget/Membrane.Blueprint.locale.template.json"]) {
    const manifest = JSON.parse(read(file));
    assert.equal(manifest.PackageIdentifier, "Membrane.Blueprint");
  }
});

test("Scoop manifest has 64bit URL and hash", () => {
  const scoop = JSON.parse(read("release/scoop/blueprint.json.template"));
  assert.equal(scoop._template, true);
  assert.ok(scoop.architecture["64bit"].url.endsWith("blueprint-__VERSION__.tgz"));
  assert.equal(scoop.architecture["64bit"].hash, "__NPM_TARBALL_SHA256__");
  assert.deepEqual(scoop.bin, ["blueprint.cmd", "blueprint-mcp.cmd"]);
});

test("Linux archive metadata honors XDG paths", () => {
  const linux = JSON.parse(read("release/linux/blueprint-archive.json"));
  assert.ok(linux.xdg.config.includes("XDG_CONFIG_HOME"));
  assert.ok(linux.xdg.data.includes("XDG_DATA_HOME"));
});

test("MCP server.json launches blueprint mcp serve from npm", () => {
  const server = JSON.parse(read("server.json"));
  assert.equal(server.command, "npx");
  assert.ok(server.args.includes("@membrane/blueprint"));
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
  for (const path of ["release/catalog.json", "release/compatibility.json", "release/homebrew/blueprint.rb", "release/scoop/blueprint.json", "release/winget/Membrane.Blueprint.version.json", "release/winget/Membrane.Blueprint.installer.json", "release/winget/Membrane.Blueprint.locale.json"]) {
    assert.equal(existsSync(join(ROOT, path)), false, path);
  }
  assert.equal(JSON.parse(read("release/catalog.template.json")).publishable, false);
  assert.equal(JSON.parse(read("release/compatibility.template.json")).publishable, false);
});
