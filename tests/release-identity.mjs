import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { adoptionArgs, platformFor } from "../scripts/stage-binaries.mjs";

test("adoption is lock-based & stages only into src-tauri/addons", () => {
  const args = adoptionArgs({
    platform: "mac",
    lock: "/tmp/membrane.lock.json",
    output: "/tmp/orthic/src-tauri/addons",
    source: "/tmp/sealed",
  });
  assert.deepEqual(args, [
    "exec", "right-release", "addon", "adopt",
    "--lock", "/tmp/membrane.lock.json",
    "--platform", "mac",
    "--output", "/tmp/orthic/src-tauri/addons",
    "--source", "/tmp/sealed",
  ]);
  assert.equal(platformFor("win32"), "win");
  const lock = JSON.parse(readFileSync(new URL("../product-addons/membrane.lock.json", import.meta.url), "utf8"));
  assert.equal(lock.version, "0.1.0");
  for (const platform of ["mac", "win"]) {
    assert.match(lock.targets[platform].manifestUrl, new RegExp(`^https://github\\.com/Orthic-Labs/Membrane/releases/download/addon-membrane-v0\\.1\\.0-${platform}-sha256-${lock.targets[platform].manifestSha256}/addon-manifest\\.json$`));
  }
});

test("frontend build has no product checkout or binary staging dependency", () => {
  const content = readFileSync(new URL("../scripts/build-frontend.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(content, /ORTHIC_PRODUCT_BINARIES_DIR|orthic-product-binaries|apps\/membrane-hub|engine\/target/);
});

test("release config preserves dual installer routes & excludes crypt", async () => {
  const cfg = (await import(`../right-release.config.mjs?test=${Date.now()}`)).default;
  assert.equal(cfg.app, "orthic");
  assert.equal(cfg.version, "0.1.11");
  assert.equal(cfg.targets.win.signingContract, "windows-raw-exe-authenticode-before-nsis-v1");
  assert.deepEqual(cfg.targets.win.nsisUpgradeContract, {});
  for (const target of [cfg.targets.mac, cfg.targets.win]) {
    assert.ok(target.installer.artifacts.some(({ key }) => key.includes("cortex")));
    assert.ok(target.installer.artifacts.some(({ key }) => key.includes("membrane")));
  }
  const tauri = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.deepEqual(tauri.bundle.externalBin, [
    "addons/membrane/bin/crypt-service",
    "addons/membrane/bin/membrane",
  ]);
});

test("release prep contains no parent workspace or gitlink access", () => {
  for (const file of ["stage-binaries.mjs", "build-mac-release.mjs", "build-windows-release.mjs"]) {
    const content = readFileSync(new URL(`../scripts/${file}`, import.meta.url), "utf8");
    assert.doesNotMatch(content, /parentWorkspace|gitlink|release-cache|\.\.\/\.\.\/membrane/);
  }
  const adoption = readFileSync(new URL("../scripts/stage-binaries.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(adoption, /copyFile|renameSync/, "Orthic must not materialize Membrane executable aliases");
});
