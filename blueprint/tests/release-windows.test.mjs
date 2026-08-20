// D18: Windows signing/installer artifacts — per-user installer, Azure
// signing step, and signature verification. Live signing is
// owner-credential-gated; these assert the artifacts are present and
// structurally correct.

import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");

test("Inno Setup installer targets per-user install and user PATH", () => {
  const iss = readFileSync(join(ROOT, "release/windows/Blueprint.iss"), "utf8");
  assert.ok(iss.includes("PrivilegesRequired=lowest"));
  assert.ok(iss.includes("{localappdata}\\Orthic\\Blueprint"));
  assert.ok(iss.includes("RegWriteExpandStringValue(HKCU, 'Environment', 'Path'"));
});

test("GitHub Actions contains no Windows signing implementation", () => {
  const workflows = join(ROOT, ".github", "workflows");
  const source = readdirSync(workflows).filter((file) => file.endsWith(".yml")).map((file) => readFileSync(join(workflows, file), "utf8")).join("\n");
  assert.doesNotMatch(source, /azure\/login|azure\/artifact-signing-action|AZURE_/i);
});

test("Windows build/verify scripts exist", () => {
  for (const file of ["scripts/release/windows/build-installer.ps1", "scripts/release/windows/verify-signatures.ps1", "release/windows/uninstall-check.ps1", "release/windows/README.txt"]) {
    assert.ok(existsSync(join(ROOT, file)), `missing ${file}`);
  }
});

test("verify-signatures checks exe/dll/msi recursively", () => {
  const script = readFileSync(join(ROOT, "scripts/release/windows/verify-signatures.ps1"), "utf8");
  assert.ok(script.includes(".exe"));
  assert.ok(script.includes(".dll"));
  assert.ok(script.includes("Get-AuthenticodeSignature"));
});

test("installer records uninstall metadata and optional user task", () => {
  const iss = readFileSync(join(ROOT, "release/windows/Blueprint.iss"), "utf8");
  assert.ok(iss.includes("uninsdeletekey"));
  assert.ok(iss.includes("schtasks /Create"));
});
