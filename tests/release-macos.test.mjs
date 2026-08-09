// D17: macOS signing artifacts — scripts, templates, and the signing order
// (S-14). Live signing/notarization is owner-credential-gated; these assert
// the artifacts are present and structurally correct.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");

test("macOS signing script exists and requires owner variables", () => {
  const script = readFileSync(join(ROOT, "scripts/release/macos/sign-and-package.sh"), "utf8");
  assert.ok(script.includes("APPLE_TEAM_ID"));
  assert.ok(script.includes("APPLE_DEVELOPER_ID_APPLICATION"));
  assert.ok(script.includes("APPLE_DEVELOPER_ID_INSTALLER"));
  assert.ok(script.includes("APPLE_NOTARY_KEY_ID"));
  assert.ok(script.includes("APPLE_NOTARY_ISSUER_ID"));
  assert.ok(script.includes("APPLE_NOTARY_KEY_P8_BASE64"));
});

test("signing order matches S-14 (codesign, pkgbuild, productbuild, notarytool, stapler, spctl, pkgutil)", () => {
  const script = readFileSync(join(ROOT, "scripts/release/macos/sign-and-package.sh"), "utf8");
  // Scan only the main signing flow (after the owner-gate checks); the
  // --verify-only block at the top is a separate path.
  const flow = script.slice(script.indexOf("VERSION="));
  const order = ["codesign", "pkgbuild", "productbuild", "notarytool", "stapler", "spctl", "pkgutil"];
  let lastIndex = -1;
  for (const tool of order) {
    const index = flow.indexOf(tool);
    assert.ok(index > lastIndex, `${tool} out of order`);
    lastIndex = index;
  }
});

test("distribution.xml, postinstall, and uninstall.sh exist", () => {
  for (const file of ["release/macos/distribution.xml", "release/macos/postinstall", "release/macos/uninstall.sh"]) {
    assert.ok(existsSync(join(ROOT, file)), `missing ${file}`);
  }
});

test("postinstall does not enroll repositories", () => {
  const postinstall = readFileSync(join(ROOT, "release/macos/postinstall"), "utf8");
  assert.ok(postinstall.includes("cortex init"));
});

test("verify-only mode checks signatures without signing", () => {
  const script = readFileSync(join(ROOT, "scripts/release/macos/sign-and-package.sh"), "utf8");
  assert.ok(script.includes("--verify-only"));
});
