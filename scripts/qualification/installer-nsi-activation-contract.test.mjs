import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// Section Install must lay versions/<v> down and activate the release directly.
// The NSIS template cannot be compiled locally, so this asserts the structural
// contract the reviewer relies on: one `membrane.exe activate` ExecWait, no
// embedded RightRelease install.ps1 payload, and powershell.exe used only for
// the single Expand-Archive line.
const nsi = readFileSync(
  new URL("../../apps/membrane-hub/src-tauri/windows/installer.nsi", import.meta.url),
  "utf8",
);

function sectionInstall(text) {
  const start = text.indexOf("\nSection Install");
  assert.ok(start >= 0, "Section Install is present");
  const end = text.indexOf("\nSectionEnd", start);
  assert.ok(end > start, "Section Install is terminated");
  return text.slice(start, end);
}

const body = sectionInstall(nsi);
const lines = body.split(/\r?\n/).filter((line) => !line.trim().startsWith(";"));

test("Section Install activates through exactly one membrane.exe activate ExecWait", () => {
  const activate = lines.filter((line) => /ExecWait\b/.test(line) && /membrane\.exe\$"\s+activate\b/.test(line));
  assert.equal(activate.length, 1, activate.join(" | "));
  assert.match(activate[0], /activate --install-root \$"\$INSTDIR\\current\$"/);
});

test("Section Install no longer references the RightRelease install.ps1 payload", () => {
  assert.doesNotMatch(body, /install\.ps1/i);
});

test("Section Install uses powershell.exe only for the single Expand-Archive line", () => {
  const psLines = lines.filter((line) => /powershell(\.exe)?/i.test(line));
  assert.equal(psLines.length, 1, psLines.join(" | "));
  assert.match(psLines[0], /Expand-Archive/);
  assert.doesNotMatch(psLines[0], /Import-Module/);
});

test("Section Install records a step-level failure log before aborting", () => {
  assert.match(body, /FileOpen \$9 "\$INSTDIR\\logs\\install-\$\{VERSION\}\.log" a/);
  assert.match(body, /FileWrite \$9 "\$R1 exit=\$R0/);
});
