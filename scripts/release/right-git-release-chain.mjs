import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { verifyNsisEmbeddedBinary } from "@rightkit/release/nsis-payload.mjs";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const hub = join(repo, "apps", "membrane-hub");
const version = JSON.parse(readFileSync(join(hub, "package.json"), "utf8")).version;
const sourceRevision = process.env.RIGHT_GIT_SOURCE_REVISION;
const unsignedWindows = process.env.RIGHT_GIT_UNSIGNED_CANDIDATE_ROOT;
const unsignedMac = process.env.RIGHT_GIT_UNSIGNED_CANDIDATE_ROOT;
const finalizedWindows = process.env.RIGHT_GIT_FINALIZED_WINDOWS_ROOT;
const finalizedMac = process.env.RIGHT_GIT_FINALIZED_MACOS_ROOT;
const qualification = process.env.RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT;

function run(command, args, cwd = repo, env = process.env) {
  const result = spawnSync(command, args, { cwd, env, stdio: "inherit", shell: process.platform === "win32" && command.endsWith(".cmd"), windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited ${result.status}`);
}

function output(command, args, cwd = repo) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed`);
  return result.stdout.trim();
}

function sha256(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }
function ensureDirectory(path, label) { if (!path) throw new Error(`${label} is required`); mkdirSync(path, { recursive: true }); }
function copyTree(source, destination) { cpSync(source, destination, { recursive: true, force: true }); }
function onlyInstaller(root) {
  const files = readdirSync(root).filter((name) => /-setup\.exe$/i.test(name));
  if (files.length !== 1) throw new Error(`expected exactly one Windows installer in ${root}; found ${files.length}`);
  return join(root, files[0]);
}
function onlyMacDmg(root) {
	const files = readdirSync(root).filter((name) => /\.dmg$/i.test(name));
	if (files.length !== 1) throw new Error(`expected exactly one macOS DMG in ${root}; found ${files.length}`);
	return join(root, files[0]);
}
function assertSource() {
  const head = output("git", ["rev-parse", "HEAD"]);
  if (!/^[a-f0-9]{40}$/i.test(sourceRevision ?? "") || head !== sourceRevision) throw new Error("RightGit release chain source revision does not match checkout");
  if (output("git", ["status", "--porcelain"])) throw new Error("RightGit release chain requires clean source");
  return head;
}

function finalizeWindows() {
  if (process.platform !== "win32") throw new Error("Windows finalization requires native Windows host");
  assertSource();
  if (!unsignedWindows || !existsSync(join(unsignedWindows, "candidate.json"))) throw new Error("exact unsigned Windows candidate is required");
  const candidate = JSON.parse(readFileSync(join(unsignedWindows, "candidate.json"), "utf8"));
  if (candidate.target !== "windows-x86_64" || candidate.sourceCommit !== sourceRevision) throw new Error("unsigned Windows candidate identity mismatch");
  run("pnpm.cmd", ["--dir", hub, "install", "--frozen-lockfile"]);
  ensureDirectory(finalizedWindows, "RIGHT_GIT_FINALIZED_WINDOWS_ROOT");
  rmSync(finalizedWindows, { recursive: true, force: true });
  mkdirSync(finalizedWindows, { recursive: true });
  run("pnpm.cmd", ["--dir", hub, "run", "release:build:portable:win"], repo, { ...process.env, MEMBRANE_CANDIDATE_ROOT: unsignedWindows });
  const portable = join(hub, "dist", "portable");
  if (!existsSync(portable)) throw new Error("RightKit Windows finalization did not produce portable release inputs");
  const installer = onlyInstaller(portable);
  const embeddedReceiptPath = join(portable, "nsis-embedded-receipt.json");
  if (!existsSync(embeddedReceiptPath)) throw new Error("NSIS embedded-release receipt is required before outer signing");
  const embeddedReceipt = JSON.parse(readFileSync(embeddedReceiptPath, "utf8"));
  if (embeddedReceipt.contract !== "membrane-nsis-direct-release-embedding-v1" || embeddedReceipt.installerSha256 !== sha256(installer) || !Array.isArray(embeddedReceipt.embedded) || embeddedReceipt.embedded.length < 7) throw new Error("NSIS embedded-release receipt does not bind unsigned outer installer");
  run("pnpm.cmd", ["--dir", hub, "exec", "right-release", "sign-windows", installer]);
  run("pnpm.cmd", ["--dir", hub, "exec", "right-release", "sign-windows", "--verify-only", installer]);
  const postSignEmbedded = embeddedReceipt.embedded.map((entry) => verifyNsisEmbeddedBinary({ installer, entryName: entry.entry, expectedSha256: entry.sha256 }));
  const manifest = JSON.parse(readFileSync(join(unsignedWindows, "release-manifest.json"), "utf8"));
  const sbom = JSON.parse(readFileSync(join(unsignedWindows, "sbom.json"), "utf8"));
  const installerSha256 = sha256(installer);
  const signing = { status: "signed", contract: "azure-artifact-signing-v1", provider: "RightRelease" };
  const installerSize = statSync(installer).size;
  manifest.artifact = { ...manifest.artifact, path: installer.split(/[\\/]/).pop(), sha256: installerSha256, size: installerSize };
  manifest.release = { ...manifest.release, artifact_sha256: installerSha256 };
  manifest.signing = signing;
  sbom.artifact = { ...sbom.artifact, path: installer.split(/[\\/]/).pop(), sha256: installerSha256, size: installerSize };
  sbom.signing = signing;
  copyTree(installer, join(finalizedWindows, installer.split(/[\\/]/).pop()));
  writeFileSync(join(finalizedWindows, "release-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(join(finalizedWindows, "sbom.json"), `${JSON.stringify(sbom, null, 2)}\n`);
  writeFileSync(join(finalizedWindows, "nsis-embedded-receipt.json"), `${JSON.stringify({ ...embeddedReceipt, signedInstallerSha256: installerSha256, embedded: postSignEmbedded }, null, 2)}\n`);
  copyTree(portable, join(finalizedWindows, "portable"));
}

function finalizeMac() {
  if (process.platform !== "darwin") throw new Error("macOS finalization requires native macOS host");
  assertSource();
  if (!unsignedMac || !existsSync(join(unsignedMac, "candidate.json"))) throw new Error("exact unsigned macOS candidate is required");
  const candidate = JSON.parse(readFileSync(join(unsignedMac, "candidate.json"), "utf8"));
  if (candidate.target !== "macos-arm64" || candidate.sourceCommit !== sourceRevision) throw new Error("unsigned macOS candidate identity mismatch");
  run("pnpm", ["--dir", hub, "install", "--frozen-lockfile"]);
  ensureDirectory(finalizedMac, "RIGHT_GIT_FINALIZED_MACOS_ROOT");
  rmSync(finalizedMac, { recursive: true, force: true });
  mkdirSync(finalizedMac, { recursive: true });
  run("pnpm", ["--dir", hub, "run", "release:build:mac"], repo, { ...process.env, MEMBRANE_PUBLIC_CI_DIRECT_CARGO: "1" });
  const metadata = JSON.parse(output("cargo", ["metadata", "--format-version", "1", "--no-deps", "--manifest-path", "apps/membrane-hub/src-tauri/Cargo.toml"]));
  const dmg = join(metadata.target_directory, "aarch64-apple-darwin", "release", "bundle", "dmg", `Membrane Hub_${version}_aarch64.dmg`);
  if (!existsSync(dmg)) throw new Error(`signed macOS DMG is missing: ${dmg}`);
  const name = `Membrane_Hub_${version}_arm64.dmg`;
  cpSync(dmg, join(finalizedMac, name));
  writeFileSync(join(finalizedMac, "finalization.json"), `${JSON.stringify({ schemaVersion: 1, target: "macos-arm64", sourceRevision, candidateArchive: candidate.archive, artifact: { name, sha256: sha256(dmg) }, notarized: true, stapled: true }, null, 2)}\n`);
}

function qualifyInstalled() {
  if (process.platform !== "win32") throw new Error("installed qualification requires protected native Windows host");
  assertSource();
  ensureDirectory(qualification, "RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT");
  const installer = onlyInstaller(finalizedWindows);
  const manifest = join(finalizedWindows, "release-manifest.json");
  const sbom = join(finalizedWindows, "sbom.json");
  const releases = JSON.parse(output("gh", ["api", "repos/Orthic-Labs/Membrane/releases?per_page=100"]));
  const prior = releases.find((release) => !release.draft && !release.prerelease && release.tag_name !== `v${version}` && /^v0\.1\.(?:1[89]|[2-9]\d|\d{3,})$/.test(release.tag_name));
  const asset = prior?.assets?.find((entry) => /-setup\.exe$/i.test(entry.name));
  if (version !== "0.1.18" && (!prior || !asset)) throw new Error("installed qualification requires one prior stable-layout signed Windows installer");
  const args = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/qualification/install-release.ps1", "-Installer", installer, "-ReleaseManifest", manifest, "-Sbom", sbom, "-EvidencePath", join(qualification, "evidence.json")];
  if (prior && asset) {
    if (asset.name !== asset.name.split(/[\\/]/).pop()) throw new Error("prior signed Windows release installer name is unsafe");
    const previous = join(qualification, "previous-signed-installer.exe");
    const downloaded = join(qualification, asset.name);
    run("gh", ["release", "download", prior.tag_name, "--repo", "Orthic-Labs/Membrane", "--pattern", asset.name, "--dir", qualification, "--clobber"]);
    if (!existsSync(downloaded)) throw new Error("prior signed Windows release installer download is missing");
    cpSync(downloaded, previous, { force: true });
    args.push("-PreviousInstaller", previous);
  }
  run("powershell", args);
}

function publishQualified() {
  if (process.platform !== "win32") throw new Error("qualified publication requires protected native Windows host");
  assertSource();
  if (!existsSync(join(qualification, "evidence.json"))) throw new Error("installed qualification evidence is required before publication");
  const portable = join(finalizedWindows, "portable");
  if (!existsSync(portable)) throw new Error("finalized portable release inputs are required before publication");
  const destination = join(hub, "dist", "portable");
  rmSync(destination, { recursive: true, force: true });
  copyTree(portable, destination);
  run("pnpm.cmd", ["--dir", hub, "run", "release:publish:portable:win"]);
  const installer = onlyInstaller(finalizedWindows);
  run("gh", ["release", "upload", `v${version}`, installer, "--clobber"]);
  const dmg = onlyMacDmg(finalizedMac);
  const finalizationPath = join(finalizedMac, "finalization.json");
  if (!existsSync(finalizationPath)) throw new Error("macOS finalization receipt is required before publication");
  const finalization = JSON.parse(readFileSync(finalizationPath, "utf8"));
  if (finalization.target !== "macos-arm64" || finalization.sourceRevision !== sourceRevision || finalization.notarized !== true || finalization.stapled !== true || finalization.artifact?.name !== dmg.split(/[\\/]/).pop() || finalization.artifact?.sha256 !== sha256(dmg)) {
    throw new Error("macOS finalization receipt does not bind the exact notarized & stapled DMG");
  }
  run("gh", ["release", "upload", `v${version}`, dmg, finalizationPath, "--clobber"]);
}

const mode = process.argv[2];
if (mode === "finalize-windows") finalizeWindows();
else if (mode === "finalize-macos") finalizeMac();
else if (mode === "qualify-installed") qualifyInstalled();
else if (mode === "publish-qualified") publishQualified();
else throw new Error("usage: right-git-release-chain.mjs <finalize-windows|finalize-macos|qualify-installed|publish-qualified>");
