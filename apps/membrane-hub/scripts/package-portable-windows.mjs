import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") throw new Error("portable Windows package must run on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const pkg = JSON.parse(readFileSync(join(hub, "package.json"), "utf8"));
const hubArg = process.argv.indexOf("--hub-exe");
if (hubArg < 0 || !process.argv[hubArg + 1]) {
  throw new Error("usage: package-portable-windows.mjs --hub-exe <signed membrane-hub.exe>");
}

const output = join(hub, "dist", "portable");
const payload = join(output, "membrane-windows-x64");
const archiveName = "membrane-windows-x64.zip";
const archive = join(output, archiveName);
const executables = [
  [resolve(process.argv[hubArg + 1]), "membrane-hub.exe"],
  [join(hub, "src-tauri", "binaries", "cortex-x86_64-pc-windows-msvc.exe"), "cortex.exe"],
  [join(hub, "src-tauri", "binaries", "membrane-x86_64-pc-windows-msvc.exe"), "membrane.exe"],
  [join(hub, "src-tauri", "binaries", "membrane-tray-x86_64-pc-windows-msvc.exe"), "membrane-tray.exe"],
  [join(hub, "src-tauri", "binaries", "membrane-daemon-x86_64-pc-windows-msvc.exe"), "membrane-daemon.exe"],
];

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function filesUnder(root) {
  const result = [];
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) result.push(...filesUnder(path));
    else result.push(path);
  }
  return result;
}

function powershell(script, args = []) {
  const result = spawnSync(
    "powershell.exe",
    ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script, ...args],
    { cwd: hub, encoding: "utf8", stdio: "inherit", windowsHide: true },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`PowerShell exited ${result.status}`);
}

rmSync(payload, { recursive: true, force: true });
rmSync(archive, { force: true });
mkdirSync(payload, { recursive: true });

for (const [source, name] of executables) {
  if (!existsSync(source)) throw new Error(`signed executable missing: ${source}`);
  cpSync(source, join(payload, name));
}
const runtime = join(hub, "src-tauri", "runtime");
if (!existsSync(runtime)) throw new Error(`staged runtime missing: ${runtime}`);
cpSync(runtime, join(payload, "runtime"), { recursive: true });
cpSync(join(repo, "install.ps1"), join(payload, "install.ps1"));
for (const entry of ["plugin.json", "mcp.json", "skills"]) {
  cpSync(join(repo, entry), join(payload, entry), { recursive: true });
}
cpSync(join(repo, "LICENSE"), join(payload, "LICENSE"));

powershell(
  "$ErrorActionPreference='Stop'; foreach($p in $args){ $s=Get-AuthenticodeSignature -LiteralPath $p; if($s.Status -ne 'Valid'){ throw \"invalid Authenticode signature: $p ($($s.Status))\" } }",
  executables.map(([, name]) => join(payload, name)),
);

const membraneInfo = spawnSync(join(payload, "membrane.exe"), ["cli", "build-info"], {
  encoding: "utf8",
  windowsHide: true,
});
if (membraneInfo.error || membraneInfo.status !== 0) throw new Error("membrane build-info failed");
const buildInfo = JSON.parse(membraneInfo.stdout);
const manifest = {
  schemaVersion: 1,
  product: "membrane",
  version: pkg.version,
  os: "windows",
  arch: "x64",
  releaseGeneration: buildInfo.release_generation,
  files: Object.fromEntries(
    filesUnder(payload)
      .filter((path) => basename(path) !== "release.json")
      .map((path) => [relative(payload, path).replaceAll("\\", "/"), sha256(path)]),
  ),
};
writeFileSync(join(payload, "release.json"), `${JSON.stringify(manifest, null, 2)}\n`);

powershell(
  "$ErrorActionPreference='Stop'; Compress-Archive -Path (Join-Path $args[0] '*') -DestinationPath $args[1] -CompressionLevel Optimal -Force",
  [payload, archive],
);

const archiveHash = sha256(archive);
writeFileSync(`${archive}.sha256`, `${archiveHash}  ${archiveName}\n`);
writeFileSync(
  join(output, "checksums.json"),
  `${JSON.stringify({ schemaVersion: 1, version: pkg.version, assets: { [archiveName]: { sha256: archiveHash } } }, null, 2)}\n`,
);
console.log(`${archive}\nsha256:${archiveHash}`);
