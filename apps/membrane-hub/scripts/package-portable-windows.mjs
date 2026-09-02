import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { CLIENT_PROJECTION_KINDS, assemblePortableCore, validatePortableCore } from "@rightkit/ax/plugin/portable-core";
import { createPortableArchive } from "@rightkit/release/direct-bootstrap.mjs";
import { materializeCycloneDxSbom, materializeInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";
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
const startedArg = process.argv.indexOf("--started-at");
const inputArg = process.argv.indexOf("--input-root");
if (hubArg < 0 || !process.argv[hubArg + 1] || startedArg < 0 || !process.argv[startedArg + 1]) {
  throw new Error("usage: package-portable-windows.mjs --hub-exe <signed membrane-hub.exe> --started-at <ISO-8601>");
}
const inputRoot = inputArg >= 0 ? resolve(process.argv[inputArg + 1]) : null;
const projectionRoot = inputRoot || repo;
const descriptorRoot = projectionRoot;

const output = join(hub, "dist", "portable");
const payload = join(output, `membrane-${pkg.version}-windows-x86_64`);
const portableCore = join(output, "agent-plugin-core");
const archiveName = `membrane-${pkg.version}-windows-x86_64.zip`;
const archive = join(output, archiveName);
const executables = [
  [resolve(process.argv[hubArg + 1]), "membrane-hub.exe"],
  [inputRoot ? join(inputRoot, "cortex.exe") : join(hub, "src-tauri", "binaries", "cortex-x86_64-pc-windows-msvc.exe"), "cortex.exe"],
  [inputRoot ? join(inputRoot, "membrane.exe") : join(hub, "src-tauri", "binaries", "membrane-x86_64-pc-windows-msvc.exe"), "membrane.exe"],
  [inputRoot ? join(inputRoot, "membrane-tray.exe") : join(hub, "src-tauri", "binaries", "membrane-tray-x86_64-pc-windows-msvc.exe"), "membrane-tray.exe"],
  [inputRoot ? join(inputRoot, "membrane-daemon.exe") : join(hub, "src-tauri", "binaries", "membrane-daemon-x86_64-pc-windows-msvc.exe"), "membrane-daemon.exe"],
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
rmSync(portableCore, { recursive: true, force: true });
rmSync(archive, { force: true });
mkdirSync(payload, { recursive: true });

for (const [source, name] of executables) {
  if (!existsSync(source)) throw new Error(`signed executable missing: ${source}`);
  cpSync(source, join(payload, name));
}
const runtime = inputRoot ? join(inputRoot, "runtime") : join(hub, "src-tauri", "runtime");
if (!existsSync(runtime)) throw new Error(`staged runtime missing: ${runtime}`);
cpSync(runtime, join(payload, "runtime"), { recursive: true });
const pluginContract = assemblePortableCore({
  outputDir: portableCore,
  pluginManifestPath: join(projectionRoot, "plugin.json"),
  mcpManifestPath: join(projectionRoot, "mcp.json"),
  skills: [{
    id: "membrane",
    visibility: "public",
    sourceRoot: projectionRoot,
    sourceDir: join(projectionRoot, "skills", "membrane"),
  }],
  clientProjections: CLIENT_PROJECTION_KINDS,
});
const pluginValidation = validatePortableCore(portableCore);
if (!pluginValidation.valid) throw new Error(`Agent Plugins core invalid: ${pluginValidation.errors.join("; ")}`);
for (const entry of readdirSync(portableCore)) {
  cpSync(join(portableCore, entry), join(payload, entry), { recursive: true });
}
cpSync(join(descriptorRoot, ".claude-plugin"), join(payload, ".claude-plugin"), { recursive: true });
cpSync(join(descriptorRoot, ".codex-plugin"), join(payload, ".codex-plugin"), { recursive: true });
mkdirSync(join(payload, ".agents", "skills"), { recursive: true });
cpSync(join(descriptorRoot, "skills", "membrane"), join(payload, ".agents", "skills", "membrane"), { recursive: true });
cpSync(join(descriptorRoot, ".antigravity-plugin"), join(payload, ".antigravity-plugin"), { recursive: true });
mkdirSync(join(payload, ".antigravity-plugin", "skills"), { recursive: true });
cpSync(join(descriptorRoot, "skills", "membrane"), join(payload, ".antigravity-plugin", "skills", "membrane"), { recursive: true });
// Claude hooks are product code, not a development-only source reference.
// Carry their complete, dependency-closed projection in the candidate so
// protected finalization consumes exactly what CI produced.
const hookFiles = [
  ["mcp/hooks/membrane-hook-entrypoint.mjs", "mcp/hooks/membrane-hook-entrypoint.mjs"],
  ["mcp/hooks/membrane-hook-runtime.mjs", "mcp/hooks/membrane-hook-runtime.mjs"],
  ["mcp/hooks/membrane-workspace-operations.mjs", "mcp/hooks/membrane-workspace-operations.mjs"],
  ["mcp/lib/verification-command.mjs", "mcp/lib/verification-command.mjs"],
  ["mcp/lib/diagnostics-client.mjs", "mcp/lib/diagnostics-client.mjs"],
  ["mcp/host/context-adapter.cjs", "mcp/host/context-adapter.cjs"],
  ["mcp/host/continuity.mjs", "mcp/host/continuity.mjs"],
  ["mcp/host/delivery-ledger-store.cjs", "mcp/host/delivery-ledger-store.cjs"],
  ["mcp/host/observable-event.cjs", "mcp/host/observable-event.cjs"],
  ["mcp/host/observable-ingress.cjs", "mcp/host/observable-ingress.cjs"],
  ["mcp/context-renderer-lib.cjs", "mcp/context-renderer-lib.cjs"],
];
for (const [source, destination] of hookFiles) {
  const from = join(projectionRoot, source);
  if (!existsSync(from)) throw new Error(`installed hook projection file missing: ${from}`);
  mkdirSync(join(payload, destination, ".."), { recursive: true });
  cpSync(from, join(payload, destination));
}
cpSync(join(projectionRoot, "LICENSE"), join(payload, "LICENSE"));
cpSync(join(projectionRoot, "THIRD_PARTY_NOTICES.md"), join(payload, "THIRD_PARTY_NOTICES.md"));

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
// The binary bakes its release generation at compile time from
// dist/release-identity.json. A cached compile made before that file existed
// produces a binary reporting "sha256:unknown", and the manifest, /health and
// the activation receipt then all report an unidentifiable release — which is
// exactly what shipped in 0.1.24. Refuse to package it.
if (!buildInfo.release_generation || buildInfo.release_generation.endsWith("unknown")) {
  throw new Error(
    `membrane reports release_generation ${buildInfo.release_generation}: the binary was compiled without dist/release-identity.json (often a stale Cargo cache). Rebuild the sidecars after running release:identity.`,
  );
}
{
  const identityPath = join(hub, "dist", "release-identity.json");
  if (existsSync(identityPath)) {
    const identity = JSON.parse(readFileSync(identityPath, "utf8"));
    if (identity.releaseGeneration !== buildInfo.release_generation) {
      throw new Error(
        `membrane reports release_generation ${buildInfo.release_generation} but this tree's identity is ${identity.releaseGeneration}: the packaged binary is not this source.`,
      );
    }
  }
}
const manifest = {
  schemaVersion: 1,
  product: "membrane",
  version: pkg.version,
  os: "windows",
  arch: "x64",
  releaseGeneration: buildInfo.release_generation,
  agentPlugins: pluginContract,
  files: Object.fromEntries(
    filesUnder(payload)
      .filter((path) => basename(path) !== "release.json")
      .map((path) => [relative(payload, path).replaceAll("\\", "/"), sha256(path)]),
  ),
};
writeFileSync(join(payload, "release.json"), `${JSON.stringify(manifest, null, 2)}\n`);

const archived = createPortableArchive({ sourceDir: payload, outputPath: archive });
const fileEvidence = [{ name: archiveName, sha256: archived.sha256, size: archived.size }];
const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8", windowsHide: true });
if (git.error || git.status !== 0) throw new Error("source commit resolution failed");
const sourceCommit = git.stdout.trim();
if (!/^[0-9a-f]{40}$/.test(sourceCommit)) throw new Error(`invalid source commit: ${sourceCommit}`);
const sbomPath = join(output, "sbom-windows-x86_64.cdx.json");
const provenancePath = join(output, "provenance-windows-x86_64.intoto.jsonl");
materializeCycloneDxSbom({
  outputPath: sbomPath,
  product: "membrane",
  version: pkg.version,
  target: "windows-x86_64",
  sourceCommit,
  files: fileEvidence,
});
materializeInTotoSlsaProvenance({
  outputPath: provenancePath,
  product: "membrane",
  version: pkg.version,
  target: "windows-x86_64",
  sourceCommit,
  sourceRepository: "https://github.com/Orthic-Labs/Membrane",
  subjects: fileEvidence,
  startedAt: process.argv[startedArg + 1],
});
cpSync(join(repo, "docs", "product", "legal", "THIRD-PARTY-NOTICES.txt"), join(output, "THIRD_PARTY_NOTICES.md"));
console.log(JSON.stringify({ archive, sha256: archived.sha256, provenancePath, sbomPath }));
