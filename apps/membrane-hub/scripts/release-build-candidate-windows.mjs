import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { createPortableArchive } from "@rightkit/release/direct-bootstrap.mjs";
import { materializeCycloneDxSbom, materializeInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";
import { CLIENT_PROJECTION_KINDS, assemblePortableCore, validatePortableCore } from "@rightkit/ax/plugin/portable-core";

if (process.platform !== "win32") throw new Error("Windows candidate must build on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const artifactRoot = process.env.RIGHT_GIT_ARTIFACT_ROOT;
if (!artifactRoot) throw new Error("RIGHT_GIT_ARTIFACT_ROOT is required");
const pkg = JSON.parse(readFileSync(join(hub, "package.json"), "utf8"));
const target = "x86_64-pc-windows-msvc";
const payload = join(artifactRoot, "payload");

function run(command, args, cwd = hub, env = process.env) {
  const executable = command === "pnpm" ? "pnpm.cmd" : command;
  const result = spawnSync(executable, args, { cwd, env, stdio: "inherit", shell: executable.endsWith(".cmd"), windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} exited ${result.status}`);
}

function output(command, args, cwd = repo) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed`);
  return result.stdout.trim();
}

function sha256(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }

function filesUnder(root) {
  return readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    return statSync(path).isDirectory() ? filesUnder(path) : [path];
  });
}

if (output("git", ["status", "--porcelain"])) throw new Error("candidate source must be clean");
const sourceCommit = output("git", ["rev-parse", "HEAD"]);
if (process.env.GITHUB_ACTIONS !== "true" || process.env.GITHUB_REPOSITORY !== "Orthic-Labs/Membrane" || process.env.GITHUB_SHA !== sourceCommit) {
  throw new Error("release candidates may be built only by exact-source Orthic-Labs/Membrane GitHub Actions");
}
const startedAt = new Date().toISOString();
rmSync(artifactRoot, { recursive: true, force: true });
mkdirSync(payload, { recursive: true });

run("cargo", ["build", "--locked", "--manifest-path", "../../engine/Cargo.toml", "--release", "--target", target, "-p", "cortex", "-p", "membrane", "-p", "membrane-runtime", "--bin", "cortex", "--bin", "membrane", "--bin", "membrane-daemon"]);
run("cargo", ["build", "--locked", "--manifest-path", "../membrane-tray-windows/Cargo.toml", "--release", "--target", target]);

const cargoTarget = output("cargo", ["metadata", "--locked", "--format-version", "1", "--no-deps", "--manifest-path", "engine/Cargo.toml"]);
const engineTarget = JSON.parse(cargoTarget).target_directory;
const trayTarget = JSON.parse(output("cargo", ["metadata", "--locked", "--format-version", "1", "--no-deps", "--manifest-path", "apps/membrane-tray-windows/Cargo.toml"])).target_directory;
const sidecars = [
  [join(engineTarget, target, "release", "cortex.exe"), "cortex.exe"],
  [join(engineTarget, target, "release", "membrane.exe"), "membrane.exe"],
  [join(engineTarget, target, "release", "membrane-daemon.exe"), "membrane-daemon.exe"],
  [join(trayTarget, target, "release", "membrane-tray-windows.exe"), "membrane-tray.exe"],
];
mkdirSync(join(hub, "src-tauri", "binaries"), { recursive: true });
const stagedSidecars = sidecars.map(([source, name]) => {
  if (!existsSync(source)) throw new Error(`candidate executable missing: ${source}`);
  const stagedName = name === "membrane-tray.exe" ? "membrane-tray-x86_64-pc-windows-msvc.exe" : name.replace(".exe", "-x86_64-pc-windows-msvc.exe");
  const staged = join(hub, "src-tauri", "binaries", stagedName);
  cpSync(source, staged);
  return [staged, name];
});
const candidateEnv = { ...process.env, MEMBRANE_SIDECARS_READY: "1", TAURI_ENV_TARGET_TRIPLE: target };
run("pnpm", ["run", "build"], hub, candidateEnv);
run("node", ["scripts/stage-runtime.mjs"], hub, candidateEnv);
run("pnpm", ["exec", "tauri", "build", "--target", target, "--no-bundle", "--config", "src-tauri/tauri.windows.conf.json"], hub, candidateEnv);
const hubTarget = JSON.parse(output("cargo", ["metadata", "--locked", "--format-version", "1", "--no-deps", "--manifest-path", "apps/membrane-hub/src-tauri/Cargo.toml"])).target_directory;
const hubExecutable = join(hubTarget, target, "release", "membrane-hub.exe");
if (!existsSync(hubExecutable)) throw new Error(`candidate executable missing: ${hubExecutable}`);
const signing = { status: "unsigned", reason: "public_candidate_requires_protected_finalization" };

const executables = [[hubExecutable, "membrane-hub.exe"], ...stagedSidecars];
for (const [source, name] of executables) cpSync(source, join(payload, name));
const runtime = join(hub, "src-tauri", "runtime");
if (!existsSync(runtime)) throw new Error(`candidate runtime missing: ${runtime}`);
cpSync(runtime, join(payload, "runtime"), { recursive: true });

// The candidate owns the complete installed hook projection. Protected
// finalization may sign/package these bytes, but never rebuilds or substitutes
// hook sources from a checkout.
const hookFiles = [
  "mcp/hooks/membrane-hook-entrypoint.mjs",
  "mcp/hooks/membrane-hook-runtime.mjs",
  "mcp/hooks/membrane-workspace-operations.mjs",
  "mcp/lib/verification-command.mjs",
  "mcp/lib/diagnostics-client.mjs",
  "mcp/host/context-adapter.cjs",
  "mcp/host/continuity.mjs",
  "mcp/host/delivery-ledger-store.cjs",
  "mcp/host/observable-event.cjs",
  "mcp/host/observable-ingress.cjs",
  "mcp/context-renderer-lib.cjs",
];
for (const file of hookFiles) {
  const source = join(repo, file);
  if (!existsSync(source)) throw new Error(`candidate hook projection file missing: ${source}`);
  mkdirSync(join(payload, file, ".."), { recursive: true });
  cpSync(source, join(payload, file));
}

// Candidate owns complete client/plugin projection. Protected finalization is
// forbidden from rebuilding this content from its checkout.
const portableCore = join(artifactRoot, "agent-plugin-core");
const pluginContract = assemblePortableCore({
  outputDir: portableCore,
  pluginManifestPath: join(repo, "plugin.json"),
  mcpManifestPath: join(repo, "mcp.json"),
  skills: [{ id: "membrane", visibility: "public", sourceRoot: repo, sourceDir: join(repo, "skills", "membrane") }],
  clientProjections: CLIENT_PROJECTION_KINDS,
});
const pluginValidation = validatePortableCore(portableCore);
if (!pluginValidation.valid) throw new Error(`candidate Agent Plugins core invalid: ${pluginValidation.errors.join("; ")}`);
for (const entry of readdirSync(portableCore)) cpSync(join(portableCore, entry), join(payload, entry), { recursive: true });
for (const entry of [".claude-plugin", ".codex-plugin", ".antigravity-plugin"]) cpSync(join(repo, entry), join(payload, entry), { recursive: true });
mkdirSync(join(payload, ".agents", "skills"), { recursive: true });
cpSync(join(repo, "skills", "membrane"), join(payload, ".agents", "skills", "membrane"), { recursive: true });
mkdirSync(join(payload, ".antigravity-plugin", "skills"), { recursive: true });
cpSync(join(repo, "skills", "membrane"), join(payload, ".antigravity-plugin", "skills", "membrane"), { recursive: true });
cpSync(join(repo, "LICENSE"), join(payload, "LICENSE"));
cpSync(join(repo, "docs", "product", "legal", "THIRD-PARTY-NOTICES.txt"), join(payload, "THIRD_PARTY_NOTICES.md"));

const files = Object.fromEntries(filesUnder(payload).map((path) => [relative(payload, path).replaceAll("\\", "/"), sha256(path)]));
const statusSuffix = signing.status;
const archiveName = `membrane-${pkg.version}-windows-x86_64-${statusSuffix}.zip`;
const archive = createPortableArchive({ sourceDir: payload, outputPath: join(artifactRoot, archiveName) });
const subject = [{ name: archiveName, size: archive.size, sha256: archive.sha256 }];
const sbomPath = join(artifactRoot, `sbom-windows-x86_64-${statusSuffix}.cdx.json`);
const provenancePath = join(artifactRoot, `provenance-windows-x86_64-${statusSuffix}.intoto.jsonl`);
materializeCycloneDxSbom({ outputPath: sbomPath, product: "membrane", version: pkg.version, target: "windows-x86_64", sourceCommit, files: subject });
materializeInTotoSlsaProvenance({ outputPath: provenancePath, product: "membrane", version: pkg.version, target: "windows-x86_64", sourceCommit, sourceRepository: "https://github.com/Orthic-Labs/Membrane", subjects: subject, startedAt });
const identityPath = join(hub, "dist", "release-identity.json");
if (!existsSync(identityPath)) throw new Error(`candidate release identity missing: ${identityPath}`);
const identity = JSON.parse(readFileSync(identityPath, "utf8"));
if (!/^[0-9a-f]{64}$/.test(identity.sourceTreeSha256) || identity.releaseGeneration !== `sha256:${identity.sourceTreeSha256}`) {
  throw new Error("candidate release identity is not hash-bound");
}
const releaseManifest = {
  schema: "membrane.release-evidence.v1",
  product: "Membrane Hub",
  release: {
    tag: `v${pkg.version}`,
    version: pkg.version,
    commit: sourceCommit,
    tree: identity.sourceTreeSha256,
    generation: identity.sourceTreeSha256,
    target: "windows-x86_64",
    artifact_sha256: archive.sha256,
  },
  artifact: { path: archiveName, sha256: archive.sha256, size: archive.size },
  signing,
};
const qualificationSbom = {
  schema: "membrane.sbom.v1",
  signing,
  artifact: { path: archiveName, sha256: archive.sha256, size: archive.size },
  package: { name: "membrane-hub", version: pkg.version, target: "windows-x86_64" },
  components: [{ name: "Membrane Windows candidate payload", type: "application", sha256: archive.sha256 }],
};
const releaseManifestPath = join(artifactRoot, "release-manifest.json");
const qualificationSbomPath = join(artifactRoot, "sbom.json");
writeFileSync(releaseManifestPath, `${JSON.stringify(releaseManifest, null, 2)}\n`);
writeFileSync(qualificationSbomPath, `${JSON.stringify(qualificationSbom, null, 2)}\n`);
writeFileSync(join(artifactRoot, "candidate.json"), `${JSON.stringify({
  schemaVersion: 1,
  kind: `membrane-${statusSuffix}-release-candidate`,
  product: "membrane",
  version: pkg.version,
  target: "windows-x86_64",
  signing,
  sourceCommit,
  github: { runId: process.env.GITHUB_RUN_ID, runAttempt: process.env.GITHUB_RUN_ATTEMPT },
  startedAt,
  archive: { name: basename(archive.path), size: archive.size, sha256: archive.sha256 },
  agentPlugins: pluginContract,
  releaseManifest: { name: "release-manifest.json", size: statSync(releaseManifestPath).size, sha256: sha256(releaseManifestPath) },
  sbom: { name: "sbom.json", size: statSync(qualificationSbomPath).size, sha256: sha256(qualificationSbomPath) },
  evidence: [sbomPath, provenancePath].map((path) => ({ name: basename(path), size: statSync(path).size, sha256: sha256(path) })),
  files,
}, null, 2)}\n`);
