import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { createPortableArchive } from "@rightkit/release/direct-bootstrap.mjs";
import { materializeCycloneDxSbom, materializeInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";

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

run("cargo", ["build", "--manifest-path", "../../engine/Cargo.toml", "--release", "--target", target, "-p", "cortex", "-p", "membrane", "--bin", "cortex", "--bin", "membrane"]);
run("cargo", ["build", "--manifest-path", "../../engine/Cargo.toml", "--release", "--target", target, "-p", "membrane-runtime", "--bin", "membrane-daemon"]);
run("cargo", ["build", "--manifest-path", "../membrane-tray-windows/Cargo.toml", "--release", "--target", target]);

const cargoTarget = output("cargo", ["metadata", "--format-version", "1", "--no-deps", "--manifest-path", "engine/Cargo.toml"]);
const engineTarget = JSON.parse(cargoTarget).target_directory;
const trayTarget = JSON.parse(output("cargo", ["metadata", "--format-version", "1", "--no-deps", "--manifest-path", "apps/membrane-tray-windows/Cargo.toml"])).target_directory;
const sidecars = [
  [join(engineTarget, target, "release", "cortex.exe"), "cortex.exe"],
  [join(engineTarget, target, "release", "membrane.exe"), "membrane.exe"],
  [join(engineTarget, target, "release", "membrane-daemon.exe"), "membrane-daemon.exe"],
  [join(trayTarget, target, "release", "membrane-tray-windows.exe"), "membrane-tray.exe"],
];
mkdirSync(join(hub, "src-tauri", "binaries"), { recursive: true });
for (const [source, name] of sidecars) {
  if (!existsSync(source)) throw new Error(`candidate executable missing: ${source}`);
  const stagedName = name === "membrane-tray.exe" ? "membrane-tray-x86_64-pc-windows-msvc.exe" : name.replace(".exe", "-x86_64-pc-windows-msvc.exe");
  cpSync(source, join(hub, "src-tauri", "binaries", stagedName));
}
const candidateEnv = { ...process.env, MEMBRANE_SIDECARS_READY: "1", TAURI_ENV_TARGET_TRIPLE: target };
run("pnpm", ["run", "build"], hub, candidateEnv);
run("node", ["scripts/stage-runtime.mjs"], hub, candidateEnv);
run("pnpm", ["exec", "tauri", "build", "--target", target, "--no-bundle", "--config", "src-tauri/tauri.windows.conf.json"], hub, candidateEnv);
const hubTarget = JSON.parse(output("cargo", ["metadata", "--format-version", "1", "--no-deps", "--manifest-path", "apps/membrane-hub/src-tauri/Cargo.toml"])).target_directory;
const executables = [[join(hubTarget, target, "release", "membrane-hub.exe"), "membrane-hub.exe"], ...sidecars];
for (const [source, name] of executables) {
  if (!existsSync(source)) throw new Error(`candidate executable missing: ${source}`);
  cpSync(source, join(payload, name));
}
const runtime = join(hub, "src-tauri", "runtime");
if (!existsSync(runtime)) throw new Error(`candidate runtime missing: ${runtime}`);
cpSync(runtime, join(payload, "runtime"), { recursive: true });

const files = Object.fromEntries(filesUnder(payload).map((path) => [relative(payload, path).replaceAll("\\", "/"), sha256(path)]));
const archiveName = `membrane-${pkg.version}-windows-x86_64-unsigned.zip`;
const archive = createPortableArchive({ sourceDir: payload, outputPath: join(artifactRoot, archiveName) });
const subject = [{ name: archiveName, size: archive.size, sha256: archive.sha256 }];
const sbomPath = join(artifactRoot, "sbom-windows-x86_64-unsigned.cdx.json");
const provenancePath = join(artifactRoot, "provenance-windows-x86_64-unsigned.intoto.jsonl");
materializeCycloneDxSbom({ outputPath: sbomPath, product: "membrane", version: pkg.version, target: "windows-x86_64", sourceCommit, files: subject });
materializeInTotoSlsaProvenance({ outputPath: provenancePath, product: "membrane", version: pkg.version, target: "windows-x86_64", sourceCommit, sourceRepository: "https://github.com/Orthic-Labs/Membrane", subjects: subject, startedAt });
writeFileSync(join(artifactRoot, "candidate.json"), `${JSON.stringify({
  schemaVersion: 1,
  kind: "membrane-unsigned-release-candidate",
  product: "membrane",
  version: pkg.version,
  target: "windows-x86_64",
  sourceCommit,
  github: { runId: process.env.GITHUB_RUN_ID, runAttempt: process.env.GITHUB_RUN_ATTEMPT },
  startedAt,
  archive: { name: basename(archive.path), size: archive.size, sha256: archive.sha256 },
  evidence: [sbomPath, provenancePath].map((path) => ({ name: basename(path), size: statSync(path).size, sha256: sha256(path) })),
  files,
}, null, 2)}\n`);
