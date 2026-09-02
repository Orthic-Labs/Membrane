import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// The Windows installer does four things and records each: extract the
// version tree into place and verify it, point the stable junction, register
// uninstall and shortcuts, and write one log line per step. Silent installs
// never activate; interactive installs run `membrane activate` hidden,
// synchronously and non-fatally with its output captured. The template cannot
// be compiled locally, so these tests pin the structural contract.
const nsi = readFileSync(
  new URL("../../apps/membrane-hub/src-tauri/windows/installer.nsi", import.meta.url),
  "utf8",
);

function section(text, name) {
  const start = text.indexOf(`\nSection ${name}`);
  assert.ok(start >= 0, `Section ${name} is present`);
  const end = text.indexOf("\nSectionEnd", start);
  assert.ok(end > start, `Section ${name} is terminated`);
  return text.slice(start, end);
}

const code = (body) => body.split(/\r?\n/).filter((line) => !line.trim().startsWith(";"));
const install = section(nsi, "Install");
const installLines = code(install);
const uninstall = section(nsi, "Uninstall");

test("the template never uses the invalid $\" quote form", () => {
  assert.doesNotMatch(nsi, /\$"/);
});

test("the template defines and includes what utils.nsh needs", () => {
  // utils.nsh reads these defines and uses the COM helper macros from the two
  // Win headers; run 33638254274 failed at makensis with
  // 'macro named "ComHlpr_CreateInProcInstance" not found' when they were absent.
  for (const define of ["INSTALLMODE", "ARCH", "BUNDLEID", "MAINBINARYNAME", "PRODUCTNAME"]) {
    assert.match(nsi, new RegExp(`^!define ${define} "\\{\\{[a-z_]+\\}\\}"`, "m"), define);
  }
  const includes = nsi.split(/\r?\n/).filter((line) => /^!include\b/.test(line)).map((line) => line.replace(/^!include\s+"?([^"]+)"?\s*$/, "$1"));
  for (const header of ["MUI2.nsh", "FileFunc.nsh", "x64.nsh", "Win\\COM.nsh", "Win\\Propkey.nsh", "utils.nsh"]) {
    assert.ok(includes.includes(header), `${header} included (have: ${includes.join(", ")})`);
  }
  assert.ok(includes.indexOf("Win\\Propkey.nsh") < includes.indexOf("utils.nsh"), "COM headers precede utils.nsh");
});

test("Section Install has exactly one ExecWait, the junction, and never waits on activate", () => {
  const waits = installLines.filter((line) => /ExecWait\b/.test(line));
  assert.equal(waits.length, 1, waits.join(" | "));
  assert.match(waits[0], /mklink \/J "\$INSTDIR\\\.current-next" "\$INSTDIR\\versions\\\$\{VERSION\}"/);
  assert.doesNotMatch(install, /ExecWait[^\n]*activate/);
  assert.doesNotMatch(install, /\bExec\s+'/);
});

test("interactive installs run activate hidden, captured, and non-fatal", () => {
  const start = installLines.findIndex((line) => /\$\{IfNot\}\s+\$\{Silent\}/.test(line));
  assert.ok(start >= 0, "interactive guard is present");
  const end = installLines.findIndex((line, index) => index > start && /\$\{Else\}/.test(line));
  const guarded = installLines.slice(start, end).join("\n");
  assert.match(guarded, /nsExec::ExecToStack \/TIMEOUT=\d+ '"\$INSTDIR\\current\\membrane\.exe" activate --install-root "\$INSTDIR\\current"'/);
  assert.match(guarded, /activate\.log/);
  assert.doesNotMatch(guarded, /Goto install_failed|Abort/);
  assert.match(installLines.slice(end).join("\n"), /\$\{Log\} "activate skipped \(silent install\)"/);
});

test("Section Install invokes no powershell.exe and carries no install.ps1", () => {
  assert.equal(installLines.filter((line) => /powershell(\.exe)?/i.test(line)).length, 0);
  assert.doesNotMatch(install, /install\.ps1|Expand-Archive|CopyFiles|PLUGINSDIR\\release/i);
});

test("Section Install extracts straight into the product root and verifies every executable", () => {
  assert.match(install, /SetOutPath "\$INSTDIR"\s*\n\s*ClearErrors\s*\n\s*\{\{#each resources_dirs\}\}/);
  assert.match(install, /File \/a "\/oname=\{\{this\.\[1\]\}\}" "\{\{no-escape @key\}\}"/);
  for (const exe of ["membrane.exe", "${MAINBINARYNAME}.exe", "membrane-tray.exe", "membrane-daemon.exe", "cortex.exe"]) {
    assert.ok(install.includes(`"$INSTDIR\\versions\\\${VERSION}\\${exe}"`), exe);
  }
});

test("Section Install cuts current over atomically and never recurses into a junction", () => {
  // The new junction is staged as .current-next; the live one is renamed aside
  // and the staged one renamed into place; a failed rename restores the old.
  const stage = install.indexOf('mklink /J "$INSTDIR\\.current-next" "$INSTDIR\\versions\\${VERSION}"');
  const aside = install.indexOf('Rename "$INSTDIR\\current" "$INSTDIR\\.current-previous"');
  const into = install.indexOf('Rename "$INSTDIR\\.current-next" "$INSTDIR\\current"');
  const restore = install.indexOf('Rename "$INSTDIR\\.current-previous" "$INSTDIR\\current"');
  assert.ok(stage >= 0 && aside > stage && into > aside && restore > into, "stage, aside, into, restore in order");
  assert.doesNotMatch(install, /mklink \/J "\$INSTDIR\\current"/);
  assert.match(install, /mklink[^\n]*>> "\$\{INSTALLLOG\}" 2>&1/);
  assert.match(install, /\$\{FileExists\} "\$INSTDIR\\\.current-next\\membrane\.exe"/);
  assert.match(install, /\$\{FileExists\} "\$INSTDIR\\current\\membrane\.exe"/);
  assert.doesNotMatch(install, /RMDir \/r/, "Section Install never deletes recursively");
  assert.doesNotMatch(nsi, /RMDir \/r "\$INSTDIR\\(current|\.current-next|\.current-previous)"/);
});

test("Section Install fails closed when extraction sets the error flag", () => {
  assert.match(install, /StrCpy \$InstallStep "extract-version-tree"\s*\n\s*SetOutPath "\$INSTDIR"\s*\n\s*ClearErrors/);
  const extract = install.slice(install.indexOf('"extract-version-tree"'), install.indexOf('"extract-version-tree ok"'));
  assert.match(extract, /\$\{If\} \$\{Errors\}\s*\n\s*StrCpy \$R0 1\s*\n\s*Goto install_failed/);
});

test("interactive activation is bounded", () => {
  assert.match(install, /nsExec::ExecToStack \/TIMEOUT=\d{5,6} '"\$INSTDIR\\current\\membrane\.exe" activate/);
});

test("every step is logged and a failure aborts with the step name", () => {
  for (const step of ["extract-version-tree", "verify-version-tree", "stage-next-junction", "cutover-current", "register"]) {
    assert.match(install, new RegExp(`StrCpy \\$InstallStep "${step}"`), step);
    assert.match(install, new RegExp(`\\$\\{Log\\} "${step} ok"`), `${step} ok`);
  }
  assert.match(install, /\$\{Log\} "\$InstallStep exit=\$R0"/);
  assert.match(install, /Abort "Membrane installation failed at \$InstallStep \(exit \$R0\)\. See \$\{INSTALLLOG\}"/);
  assert.match(nsi, /FileOpen \$9 "\$\{INSTALLLOG\}" a[\s\S]{0,160}FileWrite \$9 "\$\{text\}\$\\r\$\\n"/);
  assert.match(nsi, /ClearErrors\s*\n\s*FileOpen \$9 "\$\{INSTALLLOG\}" a\s*\n\s*\$\{If\} \$\{Errors\}/);
});

test("Section WebView2 logs before every abort", () => {
  const webview = section(nsi, "WebView2");
  const aborts = code(webview).filter((line) => /^\s*Abort\b/.test(line)).length;
  const logs = code(webview).filter((line) => /\$\{Log\} "webview2-[a-z]+ exit=/.test(line)).length;
  assert.ok(aborts >= 1);
  assert.equal(logs, aborts, `${logs} log lines for ${aborts} aborts`);
});

test("Section Uninstall deactivates without aborting, removes the junction before any recursive delete, and clears the product root", () => {
  assert.match(uninstall, /membrane\.exe" deactivate --install-root "\$INSTDIR\\current"[^\n]*deactivate\.log/);
  assert.doesNotMatch(uninstall, /Abort "Membrane deactivation/);
  const rm = uninstall.indexOf('RMDir "$INSTDIR\\current"');
  const guard = uninstall.indexOf("could not be removed as a junction");
  const firstRecursive = uninstall.indexOf("RMDir /r");
  assert.ok(rm >= 0 && guard > rm && firstRecursive > guard, "junction removal and guard precede every recursive delete");
  assert.match(uninstall, /Delete "\$INSTDIR\\integration-journal\.json"/);
  assert.match(uninstall, /RMDir \/r "\$INSTDIR"\n/);
  assert.doesNotMatch(uninstall, /RMDir \/r "\$(APPDATA|LOCALAPPDATA|PROFILE|TEMP)/);
  assert.match(uninstall, /DeleteRegKey HKCU "\$\{UNINSTKEY\}"/);
  assert.match(uninstall, /DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "Membrane"/);
});

test("the installer has no maintenance page and declares the in-place upgrade policy", () => {
  assert.match(nsi, /!define RIGHTKIT_AUTOMATIC_IN_PLACE_UPGRADE/);
  assert.doesNotMatch(nsi, /PageReinstall|MUI_PAGE_COMPONENTS|^\s*Page\s+custom\b/m);
});
