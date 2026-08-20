import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildManifest, checkManifest, checkSourceManifest, generate } from "./generate-install-workspace.mjs";

test("source package generates deterministic dist package and manifest", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-install-"));
  const source = join(root, "install", "workspace");
  const dist = join(root, "dist", "workspace");
  const manifestPath = join(root, "dist", "workspace-manifest.json");
  mkdirSync(source, { recursive: true });
  writeFileSync(join(source, "__init__.py"), "PACKAGE_SCHEMA = 'membrane-install-workspace-v1'\n");
  writeFileSync(join(source, "cortex_service.py"), "# current service\n");
  const first = generate({ source, dist, manifestPath });
  const second = generate({ source, dist, manifestPath });
  assert.equal(first.packageSha256, second.packageSha256);
  assert.deepEqual(checkManifest(JSON.parse(readFileSync(manifestPath, "utf8")), dist), []);
  writeFileSync(join(source, "cortex_service.py"), "# changed service\n");
  assert.deepEqual(checkSourceManifest(JSON.parse(readFileSync(manifestPath, "utf8")), source), ["workspace_source_drift", "workspace_source_manifest_mismatch"]);
});

test("manifest checker reports missing package files", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-install-"));
  const source = join(root, "source");
  const dist = join(root, "dist");
  mkdirSync(source, { recursive: true });
  writeFileSync(join(source, "cortex_service.py"), "# current service\n");
  const manifest = buildManifest(source);
  mkdirSync(dist, { recursive: true });
  assert.match(checkManifest(manifest, dist)[0], /workspace_package_missing/);
});
