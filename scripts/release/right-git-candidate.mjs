import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { createPortableArchive } from "@rightkit/release/direct-bootstrap.mjs";
import { materializeCycloneDxSbom, materializeInTotoSlsaProvenance, validateCycloneDxSbom, validateInTotoSlsaProvenance } from "@rightkit/release/supply-chain-evidence.mjs";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const hub = join(repo, "apps", "membrane-hub");
const root = process.env.RIGHT_GIT_ARTIFACT_ROOT;
const platform = process.env.RIGHT_GIT_RELEASE_PLATFORM;
const architecture = process.env.RIGHT_GIT_RELEASE_ARCHITECTURE;
const version = JSON.parse(readFileSync(join(hub, "package.json"), "utf8")).version;

function run(command, args, cwd = repo, env = process.env) {
  const result = spawnSync(command, args, { cwd, env, stdio: "inherit", shell: process.platform === "win32" && command.endsWith(".cmd"), windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited ${result.status}`);
}

function git(args) {
  const result = spawnSync("git", args, { cwd: repo, encoding: "utf8", windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`git ${args.join(" ")} failed`);
  return result.stdout.trim();
}

function output(command, args, cwd = repo) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed`);
  return result.stdout.trim();
}

function sha256(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }

function filesUnder(directory) {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    return statSync(path).isDirectory() ? filesUnder(path) : [path];
  });
}

function assertContext() {
  if (!root || !platform || !architecture) throw new Error("RightGit candidate platform, architecture, and artifact root are required");
  if (git(["status", "--porcelain"])) throw new Error("candidate source must be clean");
  const commit = git(["rev-parse", "HEAD"]);
  if (process.env.GITHUB_ACTIONS !== "true" || process.env.GITHUB_REPOSITORY !== "Orthic-Labs/Membrane" || process.env.GITHUB_SHA !== commit) {
    throw new Error("release candidates may be built only by exact-source Orthic-Labs/Membrane GitHub Actions");
  }
  return commit;
}

function buildMacCandidate() {
  if (process.platform !== "darwin" || platform !== "macos" || architecture !== "arm64") throw new Error("Membrane macOS candidate requires macos/arm64 native runner");
  const target = "aarch64-apple-darwin";
  const payload = join(root, "payload");
  rmSync(root, { recursive: true, force: true });
  mkdirSync(payload, { recursive: true });
  const env = { ...process.env, MEMBRANE_PUBLIC_CI_DIRECT_CARGO: "1", TAURI_ENV_TARGET_TRIPLE: target };
  run("pnpm", ["--dir", hub, "run", "build"], repo, env);
  run("node", ["scripts/stage-runtime.mjs"], hub, env);
  run("pnpm", ["--dir", hub, "exec", "tauri", "build", "--target", target, "--no-bundle"], repo, { ...env, MEMBRANE_SIDECARS_READY: "1" });
  const metadata = JSON.parse(output("cargo", ["metadata", "--format-version", "1", "--no-deps", "--manifest-path", "apps/membrane-hub/src-tauri/Cargo.toml"]));
  const release = join(metadata.target_directory, target, "release");
  for (const [source, name] of [[join(release, "membrane-hub"), "membrane-hub"], [join(hub, "src-tauri", "binaries", `cortex-${target}`), "cortex"], [join(hub, "src-tauri", "binaries", `membrane-${target}`), "membrane"]]) {
    if (!existsSync(source)) throw new Error(`macOS candidate executable missing: ${source}`);
    cpSync(source, join(payload, name));
  }
  const runtime = join(hub, "src-tauri", "runtime");
  if (!existsSync(runtime)) throw new Error("macOS candidate runtime missing");
  cpSync(runtime, join(payload, "runtime"), { recursive: true });
  return payload;
}

function materializeMacCandidate(commit) {
  const payload = buildMacCandidate();
  const target = "macos-arm64";
  const archiveName = `membrane-${version}-${target}-unsigned.tar.gz`;
  const archive = createPortableArchive({ sourceDir: payload, outputPath: join(root, archiveName) });
  const record = { name: archiveName, size: archive.size, sha256: archive.sha256 };
  const sbom = join(root, `sbom-${target}-unsigned.cdx.json`);
  const provenance = join(root, `provenance-${target}-unsigned.intoto.jsonl`);
  materializeCycloneDxSbom({ outputPath: sbom, product: "membrane", version, target, sourceCommit: commit, files: [record] });
  materializeInTotoSlsaProvenance({ outputPath: provenance, product: "membrane", version, target, sourceCommit: commit, sourceRepository: "https://github.com/Orthic-Labs/Membrane", subjects: [record] });
  const files = Object.fromEntries(filesUnder(payload).map((path) => [relative(payload, path).replaceAll("\\", "/"), sha256(path)]));
  writeFileSync(join(root, "candidate.json"), `${JSON.stringify({ schemaVersion: 1, kind: "membrane-unsigned-release-candidate", product: "membrane", version, target, sourceCommit: commit, archive: record, files, evidence: [{ name: basename(sbom), sha256: sha256(sbom) }, { name: basename(provenance), sha256: sha256(provenance) }] }, null, 2)}\n`);
}

function checkMacCandidate(commit) {
  const candidate = JSON.parse(readFileSync(join(root, "candidate.json"), "utf8"));
  if (candidate.kind !== "membrane-unsigned-release-candidate" || candidate.target !== "macos-arm64" || candidate.sourceCommit !== commit || candidate.version !== version) throw new Error("macOS candidate identity is invalid");
  const archive = join(root, candidate.archive?.name ?? "");
  if (!existsSync(archive) || sha256(archive) !== candidate.archive.sha256) throw new Error("macOS candidate archive digest mismatch");
  validateCycloneDxSbom(join(root, "sbom-macos-arm64-unsigned.cdx.json"), { expectedFile: candidate.archive });
  validateInTotoSlsaProvenance(join(root, "provenance-macos-arm64-unsigned.intoto.jsonl"), { expectedSubject: candidate.archive });
}

const mode = process.argv[2];
if (!new Set(["build", "check"]).has(mode)) throw new Error("usage: right-git-candidate.mjs <build|check>");
const commit = assertContext();
if (platform === "windows" && architecture === "x86_64") {
  run("pnpm.cmd", ["--dir", hub, "install", "--frozen-lockfile"]);
  run("pnpm.cmd", ["--dir", hub, "run", mode === "build" ? "release:candidate:artifacts:win" : "release:candidate:check:win"]);
} else if (platform === "macos" && architecture === "arm64") {
  run("pnpm", ["--dir", hub, "install", "--frozen-lockfile"]);
  if (mode === "build") materializeMacCandidate(commit); else checkMacCandidate(commit);
} else throw new Error(`unsupported Membrane candidate target: ${platform}/${architecture}`);
