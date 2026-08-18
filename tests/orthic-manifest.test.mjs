import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildProductManifest, validateProductManifest, writeProductManifest } from "../src/lib/init/manifest.mjs";

test("manifest builds with required SEAM §4.1 fields", () => {
  const manifest = buildProductManifest({ installRoot: process.cwd() });
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.productId, "cortex");
  assert.ok(manifest.displayName);
  assert.ok(manifest.productVersion);
  assert.ok(manifest.hubCompatRange);
  assert.ok(manifest.installRoot);
  assert.ok(Array.isArray(manifest.serviceStart));
  assert.ok(Array.isArray(manifest.serviceStop));
  assert.ok(manifest.statusEndpoint);
  assert.equal(manifest.statusEndpoint.host, "127.0.0.1");
  assert.ok(manifest.statusEndpoint.port > 0);
  assert.equal(manifest.statusEndpoint.authHeader, "Authorization");
  assert.ok(manifest.statusEndpoint.authToken);
  assert.ok(manifest.icon);
  assert.ok(manifest.icon.replaceAll("\\", "/").includes("assets/icon/cortex-tab.png"));
  assert.ok(manifest.icon.startsWith(manifest.installRoot));
  assert.ok(manifest.serviceStart[0].startsWith(manifest.installRoot));
  assert.equal(existsSync(manifest.serviceStart[0]), true);
  assert.equal(manifest.serviceStart[0].endsWith(process.platform === "win32" ? "cortex.cmd" : "cortex.mjs"), true);
});

test("manifest validates against schema", async () => {
  const manifest = buildProductManifest({ installRoot: process.cwd() });
  const schema = JSON.parse(readFileSync("schemas/orthic-product-manifest-v1.schema.json", "utf8"));
  const { default: Ajv } = await import("ajv");
  const ajv = new Ajv();
  const validate = ajv.compile(schema);
  assert.equal(validate(manifest), true, JSON.stringify(validate.errors));
});

test("manifest validates against Orthic authoritative schema", async () => {
  const { existsSync, readFileSync } = await import("node:fs");
  const { resolve } = await import("node:path");
  const canonical = resolve(process.cwd(), "../orthic/schema/manifest.v1.schema.json");
  if (!existsSync(canonical)) return;
  const { default: Ajv } = await import("ajv");
  const validate = new Ajv().compile(JSON.parse(readFileSync(canonical, "utf8")));
  const manifest = buildProductManifest({ installRoot: process.cwd() });
  assert.equal(validate(manifest), true, JSON.stringify(validate.errors));
});

test("manifest path-resolution mirrors Hub check", () => {
  const manifest = buildProductManifest({ installRoot: process.cwd() });
  const result = validateProductManifest(manifest);
  assert.equal(result.ok, true, result.errors.join(", "));
  // icon must be inside installRoot after symlink resolution
  assert.equal(manifest.icon.startsWith(manifest.installRoot), true);
});

test("writeProductManifest writes to temp location", () => {
  const tmp = mkdtempSync(join(tmpdir(), "cortex-manifest-"));
  const out = join(tmp, "cortex.json");
  try {
    const { manifest, path } = writeProductManifest({ installRoot: process.cwd(), outPath: out });
    assert.equal(path, out);
    assert.ok(existsSync(out));
    const loaded = JSON.parse(readFileSync(out, "utf8"));
    assert.equal(loaded.productId, "cortex");
    assert.equal(loaded.schemaVersion, 1);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
