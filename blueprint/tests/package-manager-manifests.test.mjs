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

test("local Homebrew & WinGet definitions are template-only", () => {
  assert.ok(existsSync(join(ROOT, "release/homebrew/blueprint.rb.template")));
  assert.equal(existsSync(join(ROOT, "release/scoop/blueprint.json.template")), false);
  const wingetRoot = join(ROOT, "release/winget");
  const version = JSON.parse(readFileSync(join(wingetRoot, "Membrane.Blueprint.version.template.json"), "utf8"));
  assert.equal(version.TEMPLATE_ONLY, true);
  assert.equal(version.publishable, false);
  assert.equal(version.packageIdentifier, "OrthicLabs.Blueprint");
  for (const file of ["Membrane.Blueprint.installer.template.yaml", "Membrane.Blueprint.locale.template.yaml"]) {
    const manifest = readFileSync(join(wingetRoot, file), "utf8");
    assert.match(manifest, /TEMPLATE_ONLY/);
    assert.match(manifest, /__VERSION__/);
    assert.doesNotMatch(manifest, /latest/i);
  }
  assert.match(readFileSync(join(wingetRoot, "Membrane.Blueprint.installer.template.yaml"), "utf8"), /v__VERSION__/);
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
  const winget = read("release/winget/Membrane.Blueprint.installer.template.yaml");
  assert.ok(!winget.includes("latest"));
});

test("tracked release tree contains templates, never final instances", () => {
  for (const path of ["release/catalog.json", "release/compatibility.json", "release/homebrew/blueprint.rb"]) {
    assert.equal(existsSync(join(ROOT, path)), false, path);
  }
  assert.equal(JSON.parse(read("release/catalog.template.json")).publishable, false);
  assert.equal(JSON.parse(read("release/compatibility.template.json")).publishable, false);
});
