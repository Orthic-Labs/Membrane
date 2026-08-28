import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const release = readFileSync(new URL("../scripts/release-build-portable-windows.mjs", import.meta.url), "utf8");
const packager = readFileSync(new URL("../scripts/package-portable-windows.mjs", import.meta.url), "utf8");
const installerUrl = new URL("../../../install.ps1", import.meta.url);
const installer = readFileSync(installerUrl, "utf8");

test("portable Windows lane signs raw app & archives without NSIS", () => {
  assert.equal(pkg.scripts["release:build:portable:win"], "node scripts/release-build-portable-windows.mjs");
  assert.match(release, /release:prepare:sidecars:win/);
  assert.match(release, /right-release", "sign-windows"/);
  assert.match(release, /rightkit:package:win", "--", "raw"/);
  assert.match(release, /right-release", "hardening"/);
  assert.match(release, /package-portable-windows\.mjs/);
  assert.doesNotMatch(release, /right-release", "build"|rightkit:package:win", "--", "package"/i);
});

test("portable payload is signed, hashed & includes activation bootstrap", () => {
  for (const name of ["membrane-hub.exe", "cortex.exe", "membrane.exe", "membrane-tray.exe", "membrane-daemon.exe", "install.ps1", "release.json", "checksums.json"]) {
    assert.ok(packager.includes(name), name);
  }
  assert.match(packager, /Get-AuthenticodeSignature/);
  assert.match(packager, /release_generation/);
  assert.match(packager, /Compress-Archive/);
  assert.match(packager, /membrane-windows-x64\.zip/);
});

test("bootstrap ports staged swap, rollback & health reconciliation", () => {
  for (const term of ["Expand-Archive", "Get-FileHash", "Get-AuthenticodeSignature", "Stop-MembraneProcesses", "Move-Item", "membrane-backup", "Invoke-Activation", "Wait-MembraneHealth", "releaseGeneration", "Codex", "Claude"]) {
    if (term === "Codex" || term === "Claude") continue;
    assert.ok(installer.includes(term), term);
  }
  assert.match(installer, /activate --install-root/);
  assert.match(installer, /releases\/latest\/download/);
  assert.match(installer, /Orthic-Labs\/Membrane/);
  assert.match(installer, /Remove-Item -LiteralPath/);
  assert.doesNotMatch(installer, /New-Service|sc\.exe|Win32_Service/);
});

test("bootstrap parses as PowerShell", { skip: process.platform !== "win32" }, () => {
  const path = fileURLToPath(installerUrl);
  const result = spawnSync("powershell.exe", [
    "-NoLogo",
    "-NoProfile",
    "-Command",
    "$e=$null;$t=$null;[System.Management.Automation.Language.Parser]::ParseFile($env:MEMBRANE_INSTALLER_PATH,[ref]$t,[ref]$e)|Out-Null;if($e.Count){$e|ForEach-Object{$_.Message};exit 1}",
  ], { encoding: "utf8", windowsHide: true, env: { ...process.env, MEMBRANE_INSTALLER_PATH: path } });
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
