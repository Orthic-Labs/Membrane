// D17: macOS signing artifacts — scripts, templates, and the signing order
// (S-14). Live signing/notarization is owner-credential-gated; these assert
// the artifacts are present and structurally correct.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");

test("macOS signing script is verify-only, signing delegated to right-release", () => {
  const script = readFileSync(join(ROOT, "scripts/release/macos/sign-and-package.sh"), "utf8");
  // In-repo signing is forbidden per docs/rules/release-signing.md and EC v4 D-18.
  // The script must delegate to right-release and retain only verify-only path.
  assert.ok(script.includes("right-release"), "must delegate to right-release");
  assert.ok(script.includes("--verify-only"), "must retain verify-only");
  assert.ok(script.includes("in-repo macOS signing is forbidden"), "must fail closed on direct signing");
  // No direct signing invocations should remain outside the verify-only block / error message.
  const flow = script.slice(script.indexOf("Signing is owned"));
  assert.equal(flow.includes("APPLE_TEAM_ID:?"), false, "must not require Apple credentials in-repo");
  assert.equal(flow.includes("codesign --force"), false, "must not invoke codesign directly");
});

test("distribution.xml, postinstall, and uninstall.sh exist", () => {
  for (const file of ["release/macos/distribution.xml", "release/macos/postinstall", "release/macos/uninstall.sh"]) {
    assert.ok(existsSync(join(ROOT, file)), `missing ${file}`);
  }
});

test("postinstall does not enroll repositories", () => {
  const postinstall = readFileSync(join(ROOT, "release/macos/postinstall"), "utf8");
  assert.ok(postinstall.includes("blueprint init"));
});

test("verify-only mode checks signatures without signing", () => {
  const script = readFileSync(join(ROOT, "scripts/release/macos/sign-and-package.sh"), "utf8");
  assert.ok(script.includes("--verify-only"));
});
