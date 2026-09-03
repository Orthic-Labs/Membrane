// Compiling the installer template locally. Two installer defects reached CI
// as failed builds because nothing here compiled the script first: a dropped
// Win header, and a `$"` sequence that is not an NSIS escape. An eleven-minute
// remote build is not a syntax checker.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const template = join(repo, "apps/membrane-hub/src-tauri/windows/installer.nsi");
const makensis = process.env.LOCALAPPDATA
  ? join(process.env.LOCALAPPDATA, "tauri", "NSIS", "makensis.exe")
  : null;

// Handlebars values that make the template a syntactically complete script.
const VALUES = {
  install_mode: "currentUser", arch: "x64", main_binary_name: "membrane-hub",
  main_binary_path: "stub.exe", product_name: "Membrane Hub", version: "0.1.24",
  manufacturer: "Orthic Labs", identifier: "com.orthic.membrane", copyright: "c",
  bundle_id: "com.orthic.membrane", short_description: "d", out_file: "out.exe",
  estimated_size: "1000", start_menu_folder: "Membrane", compression: "lzma",
  webview2_bootstrapper_path: "stub.exe", webview2_installer_path: "stub.exe",
  install_webview2_mode: "downloadBootstrapper", allow_downgrades: "true",
  display_language_selector: "false", license: "",
  installer_icon: "stub.ico", uninstaller_icon: "stub.ico",
  sidebar_image: "stub.bmp", header_image: "stub.bmp",
};

function render(directory) {
  let source = readFileSync(template, "utf8");
  // Payload loops and conditionals are data, not the syntax under test.
  source = source.replace(/\{\{#each[\s\S]*?\{\{\/each\}\}/g, "");
  source = source.replace(/\{\{#if[\s\S]*?\{\{\/if\}\}/g, "");
  const values = { ...VALUES, installer_hooks: join(directory, "hooks.nsh").replaceAll("\\", "/") };
  source = source.replace(
    /\{\{\s*(?:no-escape\s+)?([a-zA-Z_0-9.]+)[^}]*\}\}/g,
    (_match, name) => values[name.split(".").pop()] ?? "x",
  );
  // Plugin DLLs and a four-part version only exist in a real Tauri bundle run.
  source = source.replace(/^\s*(nsis_tauri_utils|nsProcess|ShellExecAsUser)::\S+.*$/gm, "  StrCpy $$1 0");
  source = source.replace(/^VIProductVersion .*$/m, 'VIProductVersion "0.1.24.0"');
  writeFileSync(join(directory, "installer.nsi"), source.replace(/\r?\n/g, "\r\n"), "utf8");
}

const skip = !makensis || !existsSync(makensis) ? "makensis is not installed on this host" : false;

test("the Windows installer template compiles", { skip }, () => {
  const directory = mkdtempSync(join(tmpdir(), "membrane-nsi-"));
  try {
    render(directory);
    cpSync(join(repo, "apps/membrane-hub/src-tauri/windows/utils.stub.nsh"), join(directory, "utils.nsh"));
    writeFileSync(join(directory, "stub.exe"), "");
    writeFileSync(join(directory, "hooks.nsh"), "");
    // Minimal valid 1x1 icon and bitmap: MUI refuses to load empty files.
    writeFileSync(join(directory, "stub.ico"), Buffer.from(
      "AAABAAEAAQEAAAEAIAAwAAAAFgAAACgAAAABAAAAAgAAAAEAIAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "base64"));
    writeFileSync(join(directory, "stub.bmp"), Buffer.from(
      "Qk06AAAAAAAAADYAAAAoAAAAAQAAAAEAAAABABgAAAAAAAQAAAATCwAAEwsAAAAAAAAAAAAAAAAA", "base64"));
    const result = spawnSync(makensis, ["/INPUTCHARSET", "UTF8", "/V2", "/XOutFile out.exe", "installer.nsi"], {
      cwd: directory, encoding: "utf8", windowsHide: true,
    });
    const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    const rendered = readFileSync(join(directory, "installer.nsi"), "utf8").split(/\r?\n/);
    // A bare line number refers to the rendered script, so quote the line too.
    const errors = output
      .split(/\r?\n/)
      .filter((line) => /^Error/i.test(line))
      .map((line) => {
        const at = /on line (\d+)/.exec(line);
        return at ? `${line} | rendered: ${rendered[Number(at[1]) - 1]}` : line;
      });
    assert.deepEqual(errors, [], `makensis rejected the template:\n${output}`);
    assert.ok(existsSync(join(directory, "out.exe")), "makensis produced no installer");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
