import test from "node:test";
import assert from "node:assert/strict";
import { SIDECAR_NAMES } from "../scripts/release-assets.mjs";
import { readFileSync } from "node:fs";

test("GENERATED paths are rerooted (no apps/membrane-hub prefix, no engine/target)", () => {
  const content = readFileSync(new URL("../scripts/release-assets.mjs", import.meta.url), "utf8");
  // Ensure no old prefix remains
  assert.ok(!content.includes("apps/membrane-hub/dist"), "should not contain apps/membrane-hub");
  assert.ok(!content.includes("engine/target"), "should not contain engine/target in GENERATED");
  assert.ok(content.includes('"dist"') || content.includes("'dist'") || content.includes("GENERATED=[\"dist\""));
});

test("SIDECAR_NAMES includes expected binaries", () => {
  assert.deepEqual(SIDECAR_NAMES, ["crypt","crypt-service","membrane"]);
});

test("build-frontend uses staged binaries not cargo build", async () => {
  const content = readFileSync(new URL("../scripts/build-frontend.mjs", import.meta.url), "utf8");
  assert.match(content, /ORTHIC_PRODUCT_BINARIES_DIR/);
  assert.doesNotMatch(content, /cargo build.*--manifest-path.*engine/);
  assert.doesNotMatch(content, /\.\.\/\.\.\/\.\.\/engine/);
});

test("right-release config has dual publish targets", async () => {
  const cfg = (await import("../right-release.config.mjs")).default;
  assert.equal(cfg.app, "orthic");
  assert.ok(cfg.targets.mac.installer.artifacts.some(a => a.key.includes("cortex")));
  assert.ok(cfg.targets.mac.installer.artifacts.some(a => a.key.includes("membrane")));
  assert.ok(!cfg.buildInputs.include.some(p => p.includes("../../engine")));
});
