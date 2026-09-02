import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { appendFileSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
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
// Installed qualification was removed from the pipeline on 2026-09-02 (it needs
// a desktop the hosted runner does not have), so this root is no longer set by
// the release workflow. Publication still needs a scratch directory to
// re-download and hash-check what it uploaded; fall back to the runner temp.
const qualification = process.env.RIGHT_GIT_QUALIFICATION_EVIDENCE_ROOT
  ?? join(process.env.RUNNER_TEMP ?? tmpdir(), "right-git-release", "qualification");
const GH_REPO = "Orthic-Labs/Membrane";

// Stages that must each have a SUCCEEDED finalize stage-summary, for this exact
// version + source revision + run, before publication is allowed to proceed.
// Stage identifiers match RIGHT_GIT_STAGE values emitted by the RightKit release
// chain generator; "candidate" appears once per matrix leg (both are required).
export const REQUIRED_RELEASE_STAGES = [
  { stage: "candidate", platform: "windows", architecture: "x86_64" },
  { stage: "candidate", platform: "macos", architecture: "arm64" },
  { stage: "windows-sign", platform: "windows", architecture: "x86_64" },
  { stage: "macos-sign", platform: "macos", architecture: "arm64" },
];

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
function windowsInstallerAsset(release) {
  return release?.assets?.find((entry) => /-setup\.exe$/i.test(entry.name));
}
function listReleases() {
  return JSON.parse(output("gh", ["api", `repos/${GH_REPO}/releases?per_page=100`]));
}
// The semver floor at which the stable Windows installer layout was introduced.
// This is a fixed historical fact about the product, never a comparison against
// the *current* release version — it must not be reintroduced as a special case
// on "am I building 0.1.18" logic.
const STABLE_INSTALLER_LAYOUT_FLOOR = { major: 0, minor: 1, patch: 18 };
function parseSemverTag(tagName) {
  const match = /^v(\d+)\.(\d+)\.(\d+)$/.exec(tagName ?? "");
  return match ? { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3]) } : null;
}
function compareSemver(a, b) {
  return a.major - b.major || a.minor - b.minor || a.patch - b.patch;
}
function isStableInstallerLayout(tagName) {
  const parsed = parseSemverTag(tagName);
  return parsed !== null && compareSemver(parsed, STABLE_INSTALLER_LAYOUT_FLOOR) >= 0;
}
// State-based prior-installer selection: walk eligible stable-layout releases
// newest-first (the GitHub releases API already returns them in that order) and
// select the newest one that actually carries a *-setup.exe asset. A release/tag
// without an installer is skipped, never treated as the answer and never treated
// as a hard stop — so an older eligible release with an installer is still found
// even when the newest eligible tag has none. Returns null when no stable-layout
// installer has ever been published (first-release same-version repair path).
// Eligibility is a real numeric semver comparison (>= STABLE_INSTALLER_LAYOUT_FLOOR),
// not a version-pinned regex, so it keeps matching 0.2.x, 1.0.0, & beyond forever.
export function selectPriorInstallerRelease(releases, currentVersion) {
  return releases.find((release) => !release.draft && !release.prerelease && release.tag_name !== `v${currentVersion}` && isStableInstallerLayout(release.tag_name) && windowsInstallerAsset(release)) ?? null;
}
function assertSource() {
  const head = output("git", ["rev-parse", "HEAD"]);
  if (!/^[a-f0-9]{40}$/i.test(sourceRevision ?? "") || head !== sourceRevision) throw new Error("RightGit release chain source revision does not match checkout");
  if (output("git", ["status", "--porcelain"])) throw new Error("RightGit release chain requires clean source");
  return head;
}

// The admission job checks out `ref: source_revision` with fetch-depth 0, so
// HEAD *is* the SHA under test — comparing against HEAD would be tautological
// and would never reject anything. Ancestry must be proven against the real
// remote main ref instead, resolving/fetching it explicitly if it is not yet
// present locally (fetch-depth 0 brings full history, but not necessarily every
// remote-tracking ref).
export function assertSourceRevisionIsAncestorOfMain(revision, { cwd = repo, remote = "origin", branch = "main" } = {}) {
  const mainRef = `${remote}/${branch}`;
  if (spawnSync("git", ["rev-parse", "--verify", "-q", mainRef], { cwd, windowsHide: true }).status !== 0) {
    const fetch = spawnSync("git", ["fetch", remote, branch], { cwd, windowsHide: true });
    if (fetch.status !== 0) throw new Error(`release chain admission could not fetch ${remote} ${branch} to verify ancestry`);
    if (spawnSync("git", ["rev-parse", "--verify", "-q", mainRef], { cwd, windowsHide: true }).status !== 0) throw new Error(`release chain admission could not resolve ${mainRef} after fetch`);
  }
  const ancestor = spawnSync("git", ["merge-base", "--is-ancestor", revision, mainRef], { cwd, windowsHide: true });
  if (ancestor.status !== 0) throw new Error("release chain admission source revision is not an ancestor of main");
}

function tagOrReleaseExists(candidateVersion) {
  const tag = `v${candidateVersion}`;
  const remoteTag = spawnSync("git", ["ls-remote", "--exit-code", "--tags", "origin", `refs/tags/${tag}`], { cwd: repo, encoding: "utf8", windowsHide: true });
  if (remoteTag.status === 0 && remoteTag.stdout.trim()) return true;
  return listReleases().some((release) => release.tag_name === tag);
}

// admission — validates the SHA-bound, admission-gated dispatch envelope for the
// tag-last release chain and emits its admitted facts to $GITHUB_OUTPUT. Nothing
// public (tag/release) is created here; this only decides whether the chain may run.
export function admitRelease(env = process.env) {
  const outputPath = env.GITHUB_OUTPUT;
  if (!outputPath) throw new Error("release chain admission requires GITHUB_OUTPUT");
  // dry_run: a rehearsal dispatch from a branch that exercises packaging,
  // signing & installed qualification with publication skipped. It accepts any
  // refs/heads/* ref, skips the first-attempt, ancestor-of-main, and
  // tag/release-existence checks, and refuses to be combined with publish=true.
  const dryRun = env.RIGHT_GIT_DRY_RUN === "true";
  const ref = env.RIGHT_GIT_WORKFLOW_REF;
  if (dryRun) {
    if (!/^refs\/heads\/.+/.test(ref ?? "")) throw new Error(`release chain admission dry run requires dispatch from a branch (refs/heads/*), got: ${ref}`);
  } else if (ref !== "refs/heads/main") {
    throw new Error(`release chain admission requires dispatch from refs/heads/main, got: ${ref}`);
  }
  const runAttempt = env.RIGHT_GIT_RUN_ATTEMPT;
  if (!dryRun && (!/^\d+$/.test(runAttempt ?? "") || Number(runAttempt) !== 1)) throw new Error(`release chain admission requires the first run attempt, got: ${runAttempt}`);
  const releaseVersion = env.RIGHT_GIT_RELEASE_VERSION ?? "";
  if (!/^\d+\.\d+\.\d+$/.test(releaseVersion)) throw new Error(`release chain admission requires an exact semver release version, got: ${releaseVersion}`);
  const signedQualification = env.RIGHT_GIT_SIGNED_QUALIFICATION === "true";
  const publish = env.RIGHT_GIT_PUBLISH === "true";
  if (dryRun && publish) throw new Error("release chain admission refuses publish=true together with dry_run=true");
  const revision = env.RIGHT_GIT_SOURCE_REVISION ?? "";
  if (!/^[a-f0-9]{40}$/.test(revision)) throw new Error("release chain admission requires an exact 40-character lowercase source revision SHA");
  if (spawnSync("git", ["cat-file", "-e", `${revision}^{commit}`], { cwd: repo, windowsHide: true }).status !== 0) throw new Error("release chain admission source revision does not resolve to a known commit");
  if (!dryRun) assertSourceRevisionIsAncestorOfMain(revision);
  if (publish && !signedQualification) throw new Error("release chain admission requires signed_qualification=true whenever publish=true");
  if (!dryRun && tagOrReleaseExists(releaseVersion)) throw new Error(`release chain admission version v${releaseVersion} already has a tag or release (drafts included)`);
  for (const [key, value] of [["version", releaseVersion], ["source_revision", revision], ["signed_qualification", String(signedQualification)], ["publish", String(publish)], ["dry_run", String(dryRun)], ["artifact_suffix", dryRun ? "-dry-run" : ""]]) {
    appendFileSync(outputPath, `${key}=${value}\n`);
  }
}

function collectEvidenceFiles(root) {
  if (!root || !existsSync(root)) return [];
  return readdirSync(root, { recursive: true })
    .filter((entry) => statSync(join(root, entry)).isFile())
    .map((entry) => ({ name: entry.replaceAll("\\", "/"), sha256: sha256(join(root, entry)), size: statSync(join(root, entry)).size }));
}

// stage-summary — records one stage's identity, outcome, & evidence into
// RIGHT_GIT_STAGE_ROOT/stage-summary.json. Called with action=init when a stage
// starts (status is not yet known) and action=finalize when it ends.
function writeStageSummary() {
  const action = process.env.RIGHT_GIT_STAGE_ACTION;
  if (action !== "init" && action !== "finalize") throw new Error(`release chain stage-summary requires RIGHT_GIT_STAGE_ACTION of init or finalize, got: ${action}`);
  const stageRoot = process.env.RIGHT_GIT_STAGE_ROOT;
  if (!stageRoot) throw new Error("release chain stage-summary requires RIGHT_GIT_STAGE_ROOT");
  const stage = process.env.RIGHT_GIT_STAGE;
  if (!stage) throw new Error("release chain stage-summary requires RIGHT_GIT_STAGE");
  const releaseVersion = process.env.RIGHT_GIT_RELEASE_VERSION;
  const revision = process.env.RIGHT_GIT_SOURCE_REVISION;
  if (!releaseVersion || !revision) throw new Error("release chain stage-summary requires RIGHT_GIT_RELEASE_VERSION & RIGHT_GIT_SOURCE_REVISION");
  let status = "STARTED";
  let exitCode = null;
  if (action === "finalize") {
    status = (process.env.RIGHT_GIT_STAGE_STATUS ?? "").toUpperCase();
    if (status !== "SUCCEEDED" && status !== "FAILED") throw new Error(`release chain stage-summary requires RIGHT_GIT_STAGE_STATUS of succeeded or failed, got: ${process.env.RIGHT_GIT_STAGE_STATUS}`);
    const rawExitCode = process.env.RIGHT_GIT_STAGE_EXIT_CODE;
    if (rawExitCode === undefined || rawExitCode === "" || Number.isNaN(Number(rawExitCode))) throw new Error("release chain stage-summary finalize requires a numeric RIGHT_GIT_STAGE_EXIT_CODE");
    exitCode = Number(rawExitCode);
    if (status === "SUCCEEDED" && exitCode !== 0) throw new Error("release chain stage-summary cannot mark a nonzero exit code SUCCEEDED");
  }
  mkdirSync(stageRoot, { recursive: true });
  const summary = {
    schemaVersion: 1,
    stage,
    action,
    producer: process.env.RIGHT_GIT_STAGE_PRODUCER ?? null,
    status,
    exitCode,
    version: releaseVersion,
    sourceRevision: revision,
    platform: process.env.RIGHT_GIT_RELEASE_PLATFORM ?? null,
    architecture: process.env.RIGHT_GIT_RELEASE_ARCHITECTURE ?? null,
    runId: process.env.RIGHT_GIT_RUN_ID ?? null,
    runAttempt: process.env.RIGHT_GIT_RUN_ATTEMPT ?? null,
    evidence: collectEvidenceFiles(process.env.RIGHT_GIT_STAGE_EVIDENCE_ROOT),
    recordedAt: new Date().toISOString(),
  };
  writeFileSync(join(stageRoot, "stage-summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
}

function findStageSummaries(root) {
  if (!existsSync(root)) throw new Error(`release chain evidence-verification root is missing: ${root}`);
  return readdirSync(root, { recursive: true })
    .filter((entry) => entry.split(/[\\/]/).pop() === "stage-summary.json")
    .map((entry) => JSON.parse(readFileSync(join(root, entry), "utf8")));
}

// evidence-verification — before publication, confirms every required stage
// (candidate, signing/finalize, & installed qualification, per platform) for
// this exact version + source revision + run has a SUCCEEDED stage-summary, and
// that the finalized artifacts on disk are the exact byte-identical, hash-bound
// inputs those stage summaries & the installed-qualification evidence attest to.
function verifyEvidence() {
  const releaseVersion = process.env.RIGHT_GIT_RELEASE_VERSION;
  const revision = process.env.RIGHT_GIT_SOURCE_REVISION;
  const runId = process.env.RIGHT_GIT_RUN_ID;
  const runAttempt = process.env.RIGHT_GIT_RUN_ATTEMPT;
  const stageSummaryRoot = process.env.RIGHT_GIT_STAGE_SUMMARY_ROOT;
  if (!releaseVersion || !revision || !runId || !runAttempt || !stageSummaryRoot) throw new Error("release chain evidence-verification requires RIGHT_GIT_RELEASE_VERSION, RIGHT_GIT_SOURCE_REVISION, RIGHT_GIT_RUN_ID, RIGHT_GIT_RUN_ATTEMPT & RIGHT_GIT_STAGE_SUMMARY_ROOT");
  const summaries = findStageSummaries(stageSummaryRoot).filter((summary) => summary.action === "finalize" && summary.version === releaseVersion && summary.sourceRevision === revision && summary.runId === runId && summary.runAttempt === runAttempt);
  for (const required of REQUIRED_RELEASE_STAGES) {
    const label = required.platform ? `${required.stage} (${required.platform}/${required.architecture})` : required.stage;
    const summary = summaries.find((entry) => entry.stage === required.stage
      && (required.platform === undefined || entry.platform === required.platform)
      && (required.architecture === undefined || entry.architecture === required.architecture));
    if (!summary) throw new Error(`release chain evidence-verification is missing a required stage summary: ${label}`);
    if (summary.status !== "SUCCEEDED") throw new Error(`release chain evidence-verification stage did not succeed: ${label}`);
  }
  const installer = onlyInstaller(finalizedWindows);
  const dmg = onlyMacDmg(finalizedMac);
  const manifestPath = join(finalizedWindows, "release-manifest.json");
  if (!existsSync(manifestPath)) throw new Error("release chain evidence-verification requires the finalized Windows release manifest");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const installerSha256 = sha256(installer);
  if (manifest.signing?.status !== "signed" || manifest.artifact?.sha256 !== installerSha256) throw new Error("release chain evidence-verification Windows installer is unsigned or hash-mismatched against its release manifest");
  const finalizationPath = join(finalizedMac, "finalization.json");
  if (!existsSync(finalizationPath)) throw new Error("release chain evidence-verification requires the macOS finalization receipt");
  const finalization = JSON.parse(readFileSync(finalizationPath, "utf8"));
  const dmgSha256 = sha256(dmg);
  if (finalization.notarized !== true || finalization.stapled !== true || finalization.artifact?.sha256 !== dmgSha256) throw new Error("release chain evidence-verification macOS DMG is not notarized/stapled or is hash-mismatched against its finalization receipt");
  const result = {
    schemaVersion: 1,
    verified: true,
    version: releaseVersion,
    sourceRevision: revision,
    runId,
    runAttempt,
    windowsInstaller: { name: installer.split(/[\\/]/).pop(), sha256: installerSha256 },
    macosDmg: { name: dmg.split(/[\\/]/).pop(), sha256: dmgSha256 },
    windowsSignature: manifest.signing,
    macosNotarization: { notarized: finalization.notarized, stapled: finalization.stapled },
    verifiedAt: new Date().toISOString(),
  };
  writeFileSync(join(stageSummaryRoot, "evidence-verification.json"), `${JSON.stringify(result, null, 2)}\n`);
}

function requireVerifiedEvidence() {
  const stageSummaryRoot = process.env.RIGHT_GIT_STAGE_SUMMARY_ROOT;
  if (!stageSummaryRoot) throw new Error("release chain publication requires RIGHT_GIT_STAGE_SUMMARY_ROOT");
  const verificationPath = join(stageSummaryRoot, "evidence-verification.json");
  if (!existsSync(verificationPath)) throw new Error("release chain publication requires evidence-verification to have run & succeeded first");
  const verification = JSON.parse(readFileSync(verificationPath, "utf8"));
  if (verification.verified !== true || verification.version !== version || verification.sourceRevision !== sourceRevision) throw new Error("release chain publication evidence verification does not match this exact version & source revision");
  return verification;
}

function reVerifyUploadedAssetHash(tag, sourcePath) {
  const name = sourcePath.split(/[\\/]/).pop();
  const verifyDir = join(qualification, "publication-verify");
  rmSync(verifyDir, { recursive: true, force: true });
  mkdirSync(verifyDir, { recursive: true });
  run("gh", ["release", "download", tag, "--pattern", name, "--dir", verifyDir, "--clobber"]);
  const downloaded = join(verifyDir, name);
  if (!existsSync(downloaded) || sha256(downloaded) !== sha256(sourcePath)) throw new Error(`release chain publication uploaded asset hash mismatch: ${name}`);
}

// Deletes the tag's release only when it is still a draft — i.e. only the draft
// this same publishQualified() invocation created. admitRelease()'s
// tagOrReleaseExists() check already refuses to admit a version that already has
// any tag or release (drafts included), so no other invocation's draft can exist
// under this tag when we get here. A release that is no longer a draft (the
// un-draft step actually succeeded despite a later failure, however unlikely) is
// never deleted, because it is a public, published release at that point.
export function deleteDraftReleaseIfStillDraft(tag) {
  const view = spawnSync("gh", ["release", "view", tag, "--json", "isDraft"], { cwd: repo, encoding: "utf8", windowsHide: true });
  if (view.status !== 0) return; // no release exists under this tag — nothing to clean up
  let isDraft = false;
  try {
    isDraft = JSON.parse(view.stdout).isDraft === true;
  } catch {
    return;
  }
  if (!isDraft) return; // never delete a release that is no longer a draft
  spawnSync("gh", ["release", "delete", tag, "--yes"], { cwd: repo, windowsHide: true });
}

// Runs `action`; on any failure it best-effort deletes the draft release under
// `tag` (via `cleanup`, which never deletes an already-published release — see
// deleteDraftReleaseIfStillDraft) and then always rethrows the *original* error.
// A cleanup failure is swallowed so it can never mask the failure that caused it.
export function runWithDraftCleanupOnFailure(tag, action, cleanup = deleteDraftReleaseIfStillDraft) {
  try {
    action();
  } catch (error) {
    try {
      cleanup(tag);
    } catch {
      // best-effort cleanup only — never mask the original error below.
    }
    throw error;
  }
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
  if (embeddedReceipt.contract !== "membrane-nsis-direct-release-embedding-v1" || embeddedReceipt.installerSha256 !== sha256(installer) || !Array.isArray(embeddedReceipt.embedded)) throw new Error("NSIS embedded-release receipt does not bind unsigned outer installer");
  // The installer embeds the release files themselves; a generated install.ps1 payload no longer rides inside it.
  const embeddedEntries = new Set(embeddedReceipt.embedded.map((entry) => String(entry.entry)));
  for (const required of ["release-manifest.json", "release-manifest.cat", "checksums.json"]) {
    if (!embeddedEntries.has(required)) throw new Error(`NSIS embedded-release receipt is missing ${required}`);
  }
  if (![...embeddedEntries].some((name) => /versions\/.*\/membrane\.exe$/.test(name))) throw new Error("NSIS embedded-release receipt is missing the versioned membrane.exe");
  if (embeddedEntries.has("install.ps1")) throw new Error("NSIS embedded-release receipt must not embed a generated install.ps1 payload");
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
  const releases = listReleases();
  const prior = selectPriorInstallerRelease(releases, version);
  const asset = windowsInstallerAsset(prior);
  const args = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/qualification/install-release.ps1", "-Installer", installer, "-ReleaseManifest", manifest, "-Sbom", sbom, "-EvidencePath", join(qualification, "evidence.json")];
  if (prior && asset) {
    if (asset.name !== asset.name.split(/[\\/]/).pop()) throw new Error("prior signed Windows release installer name is unsafe");
    const previous = join(qualification, "previous-signed-installer.exe");
    const downloaded = join(qualification, asset.name);
    run("gh", ["release", "download", prior.tag_name, "--repo", GH_REPO, "--pattern", asset.name, "--dir", qualification, "--clobber"]);
    if (!existsSync(downloaded)) throw new Error("prior signed Windows release installer download is missing");
    cpSync(downloaded, previous, { force: true });
    args.push("-PreviousInstaller", previous);
  }
  run("powershell", args);
}

// publish-qualified — final publication. Runs only once candidate, signing
// (finalize), & installed qualification have all succeeded in this exact run
// (gated on the evidence-verification result). Creates a draft release first,
// uploads the exact already-signed/notarized artifacts unchanged (no rebuild),
// re-verifies hashes on what was actually uploaded, then publishes the draft —
// the sole public tag/release transition in the whole chain.
function publishQualified() {
  if (process.platform !== "win32") throw new Error("qualified publication requires protected native Windows host");
  assertSource();
  requireVerifiedEvidence();
  const portable = join(finalizedWindows, "portable");
  if (!existsSync(portable)) throw new Error("finalized portable release inputs are required before publication");
  const destination = join(hub, "dist", "portable");
  rmSync(destination, { recursive: true, force: true });
  copyTree(portable, destination);
  const installer = onlyInstaller(finalizedWindows);
  const dmg = onlyMacDmg(finalizedMac);
  const finalizationPath = join(finalizedMac, "finalization.json");
  if (!existsSync(finalizationPath)) throw new Error("macOS finalization receipt is required before publication");
  const finalization = JSON.parse(readFileSync(finalizationPath, "utf8"));
  if (finalization.target !== "macos-arm64" || finalization.sourceRevision !== sourceRevision || finalization.notarized !== true || finalization.stapled !== true || finalization.artifact?.name !== dmg.split(/[\\/]/).pop() || finalization.artifact?.sha256 !== sha256(dmg)) {
    throw new Error("macOS finalization receipt does not bind the exact notarized & stapled DMG");
  }
  const tag = `v${version}`;
  // Everything from draft creation through the un-draft is one failure unit: a
  // transient failure anywhere in here must not leave an orphan draft behind to
  // permanently burn this version number on the next attempt (the exact failure
  // mode that previously required manual cleanup across v0.1.18-v0.1.23).
  runWithDraftCleanupOnFailure(tag, () => {
    run("gh", ["release", "create", tag, "--target", sourceRevision, "--title", tag, "--draft", "--notes", `Membrane ${tag}`]);
    run("pnpm.cmd", ["--dir", hub, "run", "release:publish:portable:win"]);
    run("gh", ["release", "upload", tag, installer, "--clobber"]);
    run("gh", ["release", "upload", tag, dmg, finalizationPath, "--clobber"]);
    reVerifyUploadedAssetHash(tag, installer);
    reVerifyUploadedAssetHash(tag, dmg);
    run("gh", ["release", "edit", tag, "--draft=false"]);
  });
}

const invokedDirectly = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (invokedDirectly) {
  const mode = process.argv[2];
  if (mode === "finalize-windows") finalizeWindows();
  else if (mode === "finalize-macos") finalizeMac();
  else if (mode === "qualify-installed") qualifyInstalled();
  else if (mode === "publish-qualified") publishQualified();
  else if (mode === "admission") admitRelease();
  else if (mode === "stage-summary") writeStageSummary();
  else if (mode === "evidence-verification") verifyEvidence();
  else throw new Error("usage: right-git-release-chain.mjs <finalize-windows|finalize-macos|qualify-installed|publish-qualified|admission|stage-summary|evidence-verification>");
}
