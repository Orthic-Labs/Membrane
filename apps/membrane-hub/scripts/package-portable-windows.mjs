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
// The NSIS installer needs this exact payload as its versions/<version> tree.
// Without it the installer ships only `runtime` and lands no executable at
// all, which is how 0.1.24 could report a clean install while replacing
// nothing. --payload-dir lets the bundler stage the tree in place, and
// --payload-only stops before the archive and evidence the installer path
// does not need.
const payloadDirArg = process.argv.indexOf("--payload-dir");
const payloadOnly = process.argv.includes("--payload-only");
const payload = payloadDirArg >= 0
  ? resolve(process.argv[payloadDirArg + 1])
  : join(output, `membrane-${pkg.version}-windows-x86_64`);
const portableCore = join(output, "agent-plugin-core");
const archiveName = `membrane-${pkg.version}-windows-x86_64.zip`;
const archive = join(output, archiveName);
const executables = [
  [resolve(process.argv[hubArg + 1]), "membrane-hub.exe"],
  [inputRoot ? join(inputRoot, "cortex.exe") : join(hub, "src-tauri", "binaries", "cortex-x86_64-pc-windows-msvc.exe"), "cortex.exe"],
  [inputRoot ? join(inputRoot, "membrane.exe") : join(hub, "src-tauri", "binaries", "membrane-x86_64-pc-windows-msvc.exe"), "membrane.exe"],
  [inputRoot ? join(inputRoot, "membrane-tray.exe") : join(hub, "src-tauri", "binaries", "membrane-tray-x86_64-pc-windows-msvc.exe"), "membrane-tray.exe"],
  [inputRoot ? join(inputRoot, "membrane-client.exe") : join(hub, "src-tauri", "binaries", "membrane-client-x86_64-pc-windows-msvc.exe"), "membrane-client.exe"],
  // membrane-daemon.exe is part of the compiled candidate closure; the
  // installer removes it post-extract (retired runtime owner), but the payload
  // must carry the exact binary set the candidate check closes over.
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
// Stable installed command uses Membrane's bounded native process owner.
writeFileSync(join(payload, "blueprint.cmd"), '@echo off\r\n"%~dp0membrane-client.exe" cli blueprint %*\r\nexit /b %ERRORLEVEL%\r\n');
// Codex hook commands run under the detected user shell; PowerShell rejects
// `"path" arg` invocation syntax and nested -Command quoting cannot satisfy
// cmd, PowerShell, and Git Bash at once, so the Windows hook entry point is
// a script file invoked through the always-present powershell.exe host.
writeFileSync(join(payload, "membrane-hook.ps1"), '& "$PSScriptRoot\\membrane-client.exe" hook --wire=codex\r\nexit $LASTEXITCODE\r\n');
const pluginContract = assemblePortableCore({
  outputDir: portableCore,
  pluginManifestPath: join(projectionRoot, "plugin.json"),
  mcpManifestPath: join(projectionRoot, "mcp.json"),
  hooksManifestPath: join(projectionRoot, "hooks", "hooks.json"),
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
mkdirSync(join(payload, ".agents", "plugins"), { recursive: true });
cpSync(join(descriptorRoot, ".agents", "plugins", "marketplace.json"), join(payload, ".agents", "plugins", "marketplace.json"));
// Codex reads the plugin root `.mcp.json` (the portable-core copy carries the
// generic Claude-shape alias). Overlay it with the Codex manifest — Codex
// strips auth fields from plugin-declared MCP servers (verified on 0.144.5:
// bearerTokenEnvVar, bearer_token_env_var, env_http_headers, and literal
// bearer_token all reach the engine unauthenticated), so `bearerTokenEnvVar`
// is the declared intent for hosts that honor it while activation keeps the
// authenticated `mcp_servers.membrane` config.toml registration — and with
// the Codex-scoped hook event set (the shared hooks/hooks.json keeps the
// Claude event superset; Codex resolves ${CLAUDE_PLUGIN_ROOT} for
// compatibility).
cpSync(join(descriptorRoot, ".mcp.json"), join(payload, ".mcp.json"));
cpSync(join(descriptorRoot, "hooks", "codex-hooks.json"), join(payload, "hooks", "codex-hooks.json"));
cpSync(join(descriptorRoot, ".antigravity-plugin"), join(payload, ".antigravity-plugin"), { recursive: true });
mkdirSync(join(payload, ".antigravity-plugin", "skills"), { recursive: true });
cpSync(join(descriptorRoot, "skills", "membrane"), join(payload, ".antigravity-plugin", "skills", "membrane"), { recursive: true });
// Plugin identity/version derives from this release, never from a
// hand-maintained per-host copy.
for (const manifestName of ["plugin.json", ".claude-plugin/plugin.json", ".codex-plugin/plugin.json", ".antigravity-plugin/plugin.json"]) {
  const manifestPath = join(payload, manifestName);
  if (!existsSync(manifestPath)) continue;
  const manifestJson = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifestJson.version = pkg.version;
  writeFileSync(manifestPath, `${JSON.stringify(manifestJson, null, 2)}\n`);
}
// Post-overlay/post-stamp closure: every host-facing artifact must exist in
// the exact payload tree and agree on identity, transport, and hook target
// before the payload can be called a release candidate. This runs after all
// overlays so a stale descriptor or missing hook file cannot ship silently.
{
  const required = [
    "plugin.json", "mcp.json", ".mcp.json", "mcp_config.json",
    "hooks/hooks.json", "hooks/codex-hooks.json", "membrane-hook.ps1",
    ".claude-plugin/plugin.json", ".claude-plugin/marketplace.json",
    ".codex-plugin/plugin.json",
    ".agents/plugins/marketplace.json", ".agents/skills/membrane/SKILL.md",
    ".antigravity-plugin/plugin.json", ".antigravity-plugin/mcp_config.json",
  ];
  for (const relative of required) {
    const path = join(payload, relative);
    if (!existsSync(path)) throw new Error(`payload plugin surface missing: ${relative}`);
    if (relative.endsWith(".json")) JSON.parse(readFileSync(path, "utf8"));
  }
  for (const manifestName of ["plugin.json", ".claude-plugin/plugin.json", ".codex-plugin/plugin.json", ".antigravity-plugin/plugin.json"]) {
    const stamped = JSON.parse(readFileSync(join(payload, manifestName), "utf8"));
    if (stamped.version !== pkg.version) {
      throw new Error(`${manifestName} version ${stamped.version} != release ${pkg.version}`);
    }
  }
  // Hooks must invoke the installed client transport, never the engine binary.
  for (const hooksName of ["hooks/hooks.json", "hooks/codex-hooks.json"]) {
    const hooksJson = readFileSync(join(payload, hooksName), "utf8");
    if (!hooksJson.includes("membrane-client") && !hooksJson.includes("membrane-hook.ps1")) {
      throw new Error(`${hooksName} does not invoke membrane-client`);
    }
    if (/membrane\.exe/.test(hooksJson)) {
      throw new Error(`${hooksName} still invokes the engine binary`);
    }
  }
  // Codex hook commands must carry the strict wire flag and a shell-portable
  // Windows entry point (powershell.exe -File survives cmd, PowerShell, and
  // Git Bash alike; bare `"path" arg` is a parse error under PowerShell).
  const codexHooksJson = JSON.parse(readFileSync(join(payload, "hooks", "codex-hooks.json"), "utf8"));
  for (const groups of Object.values(codexHooksJson.hooks ?? {})) {
    for (const handler of groups.flatMap((group) => group.hooks ?? [])) {
      if (handler.type !== "command") continue;
      if (!String(handler.command ?? "").includes("--wire=codex")) {
        throw new Error("codex hook command lacks --wire=codex");
      }
      if (!String(handler.commandWindows ?? "").includes("membrane-hook.ps1")) {
        throw new Error("codex hook commandWindows must route through membrane-hook.ps1");
      }
    }
  }
  // Codex plugin MCP binding must use the verified bearer env field and the
  // loopback listener; no literal token may appear in any manifest.
  const codexMcp = JSON.parse(readFileSync(join(payload, ".mcp.json"), "utf8"));
  if (codexMcp.mcpServers?.membrane?.bearerTokenEnvVar !== "MEMBRANE_BEARER_TOKEN") {
    throw new Error("payload .mcp.json lacks bearerTokenEnvVar binding");
  }
  if (codexMcp.mcpServers?.membrane?.url !== "http://127.0.0.1:47851/mcp") {
    throw new Error("payload .mcp.json URL drifted from the installed listener");
  }
  const claudePlugin = JSON.parse(readFileSync(join(payload, ".claude-plugin", "plugin.json"), "utf8"));
  if (claudePlugin.mcpServers?.membrane?.headers?.Authorization !== "Bearer ${MEMBRANE_BEARER_TOKEN}") {
    throw new Error("Claude plugin lacks bearer header binding");
  }
  const codexPlugin = JSON.parse(readFileSync(join(payload, ".codex-plugin", "plugin.json"), "utf8"));
  if (codexPlugin.hooks !== "./hooks/codex-hooks.json" || codexPlugin.mcpServers !== "./.mcp.json") {
    throw new Error("Codex plugin manifest does not reference codex hooks/.mcp.json");
  }
}
// A prepared candidate root carries these beside the payload; the repository
// root carries LICENSE and the canonical notices under docs/product/legal.
cpSync(join(projectionRoot, "LICENSE"), join(payload, "LICENSE"));
const noticesCandidate = join(projectionRoot, "THIRD_PARTY_NOTICES.md");
const notices = existsSync(noticesCandidate)
  ? noticesCandidate
  : join(repo, "docs", "product", "legal", "THIRD-PARTY-NOTICES.txt");
if (!existsSync(notices)) throw new Error(`third-party notices missing: ${notices}`);
cpSync(notices, join(payload, "THIRD_PARTY_NOTICES.md"));

// The unsigned development lane builds an installable product with no
// certificate. Every other lane still proves each executable is signed.
if (process.env.MEMBRANE_UNSIGNED_INSTALLER !== "1") {
  powershell(
    "$ErrorActionPreference='Stop'; foreach($p in $args){ $s=Get-AuthenticodeSignature -LiteralPath $p; if($s.Status -ne 'Valid'){ throw \"invalid Authenticode signature: $p ($($s.Status))\" } }",
    executables.map(([, name]) => join(payload, name)),
  );
}

const membraneInfo = spawnSync(join(payload, "membrane-client.exe"), ["cli", "build-info"], {
  encoding: "utf8",
  windowsHide: true,
});
if (membraneInfo.error || membraneInfo.status !== 0) throw new Error("membrane build-info failed");
const buildInfo = JSON.parse(membraneInfo.stdout);
const hookAuthority = spawnSync(join(payload, "membrane-client.exe"), ["hook", "--help"], {
  encoding: "utf8",
  windowsHide: true,
  timeout: 3_000,
});
if (hookAuthority.error || hookAuthority.status !== 0) throw new Error("membrane native hook authority unavailable");
if (existsSync(join(payload, "mcp", "hooks"))) throw new Error("obsolete mcp/hooks payload is forbidden");
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

// The installer path stops here: it needs the payload tree and the identity
// guarantees above, not the archive and release evidence. Keep release.json
// in that tree so same-version repairs replace stale installed identity too.
if (payloadOnly) process.exit(0);

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
// scripts/qualification/install-release.ps1 -Profile internal-unsigned locates
// artifacts by this stdout payload (never by re-deriving signing status
// itself). Surface the unsigned/signed disposition this packaging pass
// actually produced so that route, and any caller building its
// windows-amendment-acceptance.json PKG-02 command line, can bind to the
// exact artifact this pass built without re-inspecting Authenticode state.
console.log(JSON.stringify({
  archive,
  sha256: archived.sha256,
  provenancePath,
  sbomPath,
  unsigned: process.env.MEMBRANE_UNSIGNED_INSTALLER === "1",
  profile: process.env.MEMBRANE_UNSIGNED_INSTALLER === "1" ? "internal-unsigned" : "signed-release",
}));
