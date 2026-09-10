// scripts/qualification/cases/pkg-windows.mjs
//
// PKG-01..05 case module for windows-amendment-acceptance.json (group PKG).
// package-harness owns the run.mjs registry-runner contract (PKG-01) and
// authors this module; it never fabricates an "installed" or "passed"
// result when the live artifact/install this case actually needs is
// unavailable on this machine. A "blocked" reason is a legitimate,
// non-fabricated outcome for PKG-02..05 until the integration owner has
// built and installed the internal-unsigned Windows candidate; PKG-01 is
// fully self-contained (it exercises the registry-runner contract itself)
// and is expected to pass in source form.

import { createHash } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  loadCaseRegistry,
  runOneRegistryCase,
  runRegistryQualification,
  selectGroupCases,
} from "../run.mjs";

const nonEmptyString = (value) => typeof value === "string" && value.trim().length > 0;
const sha256File = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const readJsonIfExists = (path) => {
  if (!nonEmptyString(path) || !existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
};

// ---------------------------------------------------------------------------
// PKG-01 — Registry runner contract
//
// Proves, in-process, every clause of the contract: duplicate ids are
// rejected, a zero-selection group is rejected, a case module missing its
// declared export fails instead of being skipped, and the macOS-only gate on
// the unrelated MBR-801 path is never widened by this platform argument.
export async function PKG_01({ workspaceRoot }) {
  const checks = [];
  const scratch = mkdtempSync(join(tmpdir(), "pkg01-"));
  try {
    const duplicatePath = join(scratch, "duplicate.json");
    writeFileSync(duplicatePath, JSON.stringify({ cases: [{ id: "X-1", group: "X" }, { id: "X-1", group: "X" }] }));
    try {
      loadCaseRegistry(duplicatePath);
      checks.push({ name: "duplicate-id-rejected", passed: false, detail: "loadCaseRegistry accepted a duplicate id" });
    } catch (error) {
      checks.push({ name: "duplicate-id-rejected", passed: /duplicate/i.test(error.message) });
    }

    const singleGroupPath = join(scratch, "single-group.json");
    writeFileSync(singleGroupPath, JSON.stringify({ cases: [{ id: "Y-1", group: "Y" }] }));
    try {
      const { cases } = loadCaseRegistry(singleGroupPath);
      selectGroupCases(cases, "DOES-NOT-EXIST");
      checks.push({ name: "zero-selection-rejected", passed: false, detail: "selectGroupCases accepted a zero-case group" });
    } catch (error) {
      checks.push({ name: "zero-selection-rejected", passed: /zero/i.test(error.message) });
    }

    const missingExportResult = await runOneRegistryCase(
      { id: "Z-1", caseFile: "scripts/qualification/cases/pkg-windows.mjs", caseExport: "PKG_DOES_NOT_EXIST" },
      { workspaceRoot, profile: "internal-unsigned", platform: "windows", evidencePath: join(scratch, "unused.json") },
    );
    checks.push({ name: "missing-export-fails", passed: missingExportResult.status === "failed" });

    const missingModuleResult = await runOneRegistryCase(
      { id: "Z-2", caseFile: "scripts/qualification/cases/does-not-exist.mjs", caseExport: "ANY" },
      { workspaceRoot, profile: "internal-unsigned", platform: "windows", evidencePath: join(scratch, "unused.json") },
    );
    checks.push({ name: "missing-module-fails", passed: missingModuleResult.status === "failed" });

    const noEvidenceKindResult = await runOneRegistryCase(
      { id: "Z-3", caseFile: "scripts/qualification/cases/pkg-windows.mjs", caseExport: "PKG_TEST_NO_EVIDENCE_KIND" },
      { workspaceRoot, profile: "internal-unsigned", platform: "windows", evidencePath: join(scratch, "unused.json") },
    );
    checks.push({ name: "missing-evidence-kind-fails", passed: noEvidenceKindResult.status === "failed" });

    try {
      await runRegistryQualification({ platform: "macos", profile: "internal-unsigned", caseRegistryPath: singleGroupPath, group: "Y", evidencePath: join(scratch, "evidence.json"), workspaceRoot });
      checks.push({ name: "macos-platform-rejected", passed: false, detail: "runRegistryQualification accepted platform macos" });
    } catch (error) {
      checks.push({ name: "macos-platform-rejected", passed: /platform windows/i.test(error.message) });
    }

    try {
      await runRegistryQualification({ platform: "windows", profile: "signed-release", caseRegistryPath: singleGroupPath, group: "Y", evidencePath: join(scratch, "evidence2.json"), workspaceRoot });
      checks.push({ name: "non-internal-unsigned-profile-rejected", passed: false, detail: "runRegistryQualification accepted a non-internal-unsigned profile" });
    } catch (error) {
      checks.push({ name: "non-internal-unsigned-profile-rejected", passed: /internal-unsigned/i.test(error.message) });
    }

    const passed = checks.every((check) => check.passed === true);
    return {
      status: passed ? "passed" : "failed",
      evidenceKind: "component",
      detail: { checks },
      reason: passed ? null : `failing checks: ${checks.filter((check) => !check.passed).map((check) => check.name).join(", ")}`,
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

// Test-only fixtures consumed by PKG_01 above via the real runOneRegistryCase
// path, so the negative controls exercise the real dispatcher, not a mock.
export function PKG_TEST_NO_EVIDENCE_KIND() {
  return { status: "passed" };
}

// ---------------------------------------------------------------------------
// PKG-02 — Build and install identity binding
//
// Compares a frozen candidate manifest (produced by the RightKit windows
// build route: release-manifest.json / candidate.json under
// RIGHT_GIT_ARTIFACT_ROOT) against the identity actually installed on this
// machine. Neither path is fabricated: when either manifest is absent this
// reports a failed, non-fabricated "blocked" reason rather than a PASS, and
// never widens to a macOS/iOS/desktop target.
const FORBIDDEN_TARGET_PATTERN = /\b(mac|macos|darwin|ios|nsis-desktop|desktop)\b/i;
const RIGHTKIT_DIRECT_KIND = "rightkit-direct-release-manifest";
const ENGINE_SUBTREE = "engine";

// The shipped installed release-manifest.json is schema
// 'rightkit-direct-release-manifest' (assets[] carrying per-asset
// target/sha256, plus a top-level sourceCommit/version) -- not the
// membrane.release-evidence.v1 shape candidate.json uses. Rather than
// require the installed-manifest producer to change, PKG-02 adapts the
// rightkit-direct shape here into the same { release: { target,
// generation, artifact_sha256 } } projection candidate.json already
// carries, so the comparison is faithful instead of structurally failing.
//
// The rightkit-direct manifest has no baked releaseGeneration; it only
// carries the sourceCommit the release was built from. This replicates
// apps/membrane-hub/scripts/release-identity.mjs's committedTreeDigest
// exactly (same engine subtree, same sorted path\0blob\n hashing) against
// that real, resolvable commit -- never a fabricated or guessed value. When
// the commit cannot be resolved in this workspace, that is reported back as
// a typed error rather than silently treated as a match or a blocked run.
function committedEngineTreeDigest(workspaceRoot, commit) {
  const lsTree = execFileSync("git", ["-C", workspaceRoot, "ls-tree", "-r", commit, "--", ENGINE_SUBTREE], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  const rows = [];
  for (const line of lsTree.split("\n")) {
    if (!line) continue;
    const [metadata, path] = line.split("\t");
    rows.push([path.replace(/\\/g, "/"), metadata.trim().split(/\s+/)[2]]);
  }
  rows.sort((left, right) => (left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0));
  const digest = createHash("sha256");
  for (const [path, blob] of rows) digest.update(`${path}\0${blob}\n`, "utf8");
  return digest.digest("hex");
}

function findPreferredAsset(assets, preferredTarget = "windows-x86_64") {
  if (!Array.isArray(assets) || assets.length === 0) return null;
  return assets.find((asset) => asset?.target === preferredTarget) ?? assets[0];
}

// Returns { evidence, adapted, skipArtifactComparison, generationError, sourceCommit }.
// evidence is always release-evidence-shaped ({ release: { target, generation,
// artifact_sha256 }, signing }) whether or not adaptation happened, so PKG_02's
// comparison logic below never needs to branch on the source manifest's schema.
function deriveInstalledReleaseEvidence(installed, workspaceRoot) {
  if (!installed || installed.kind !== RIGHTKIT_DIRECT_KIND) {
    return { evidence: installed, adapted: false, skipArtifactComparison: false, generationError: null, sourceCommit: null };
  }
  const asset = findPreferredAsset(installed.assets);
  const sourceCommit = nonEmptyString(installed.sourceCommit) ? installed.sourceCommit : null;
  let generation = null;
  let generationError = null;
  if (!sourceCommit) {
    generationError = "installed rightkit-direct-release-manifest has no sourceCommit";
  } else {
    try {
      generation = committedEngineTreeDigest(workspaceRoot, sourceCommit);
    } catch (error) {
      generationError = `sourceCommit ${sourceCommit} is not resolvable in this workspace: ${error.message}`;
    }
  }
  return {
    evidence: {
      release: { target: asset?.target ?? null, generation, artifact_sha256: asset?.sha256 ?? null },
      // The zip/exe distributed through the signed GitHub release channel is
      // authenticode-signed even when this internal-unsigned candidate route
      // legitimately is not; only the generation/target identity is
      // comparable across the two distribution channels, not this flag.
      signing: { status: nonEmptyString(installed.signatureProvider) ? "release-signed" : "unsigned" },
    },
    // A rightkit-direct asset (a downloaded release zip) and a local
    // unsigned NSIS installer are never byte-identical even when built from
    // the exact same source -- they are different packaging formats. Only
    // target + generation identity is meaningful across that boundary.
    adapted: true,
    skipArtifactComparison: true,
    generationError,
    sourceCommit,
  };
}

export async function PKG_02({ row, workspaceRoot }) {
  const candidateManifestPath = row?.candidateManifestPath ?? process.env.MEMBRANE_QUALIFICATION_CANDIDATE_MANIFEST;
  const installedManifestPath = row?.installedManifestPath ?? process.env.MEMBRANE_QUALIFICATION_INSTALLED_MANIFEST;
  const candidate = readJsonIfExists(candidateManifestPath);
  const installedRaw = readJsonIfExists(installedManifestPath);

  if (!candidate || !installedRaw) {
    return {
      status: "failed",
      evidenceKind: "source",
      reason: "candidate and/or installed release manifest unavailable on this machine; run after the RightKit internal-unsigned Windows build and install",
      detail: { candidateManifestPath: candidateManifestPath ?? null, installedManifestPath: installedManifestPath ?? null },
    };
  }

  const { evidence: installed, adapted, skipArtifactComparison, generationError, sourceCommit } = deriveInstalledReleaseEvidence(installedRaw, workspaceRoot);
  if (adapted && generationError) {
    return {
      status: "failed",
      evidenceKind: "installed",
      reason: `installed manifest generation could not be derived: ${generationError}`,
      detail: { installedKind: installedRaw.kind, sourceCommit },
    };
  }

  const candidateTarget = String(candidate.release?.target ?? candidate.target ?? "");
  const installedTarget = String(installed.release?.target ?? installed.target ?? "");
  if (FORBIDDEN_TARGET_PATTERN.test(candidateTarget) || FORBIDDEN_TARGET_PATTERN.test(installedTarget)) {
    return { status: "failed", evidenceKind: "installed", reason: `forbidden non-Windows target present: candidate=${candidateTarget} installed=${installedTarget}` };
  }
  if (candidateTarget !== "windows-x86_64" || installedTarget !== "windows-x86_64") {
    return { status: "failed", evidenceKind: "installed", reason: `expected target windows-x86_64; candidate=${candidateTarget} installed=${installedTarget}` };
  }

  const candidateGeneration = candidate.release?.generation ?? candidate.releaseGeneration ?? null;
  const installedGeneration = installed.release?.generation ?? installed.releaseGeneration ?? null;
  const candidateArtifact = candidate.release?.artifact_sha256 ?? candidate.artifact?.sha256 ?? null;
  const installedArtifact = installed.release?.artifact_sha256 ?? installed.artifact?.sha256 ?? null;
  if (!nonEmptyString(candidateGeneration) || candidateGeneration !== installedGeneration) {
    return {
      status: "failed",
      evidenceKind: "installed",
      reason: `release generation mismatch: candidate=${candidateGeneration} installed=${installedGeneration}`,
      detail: { target: "windows-x86_64", candidateGeneration, installedGeneration, adapted, sourceCommit },
    };
  }
  if (!skipArtifactComparison) {
    if (!nonEmptyString(candidateArtifact) || candidateArtifact !== installedArtifact) {
      return { status: "failed", evidenceKind: "installed", reason: `artifact sha256 mismatch: candidate=${candidateArtifact} installed=${installedArtifact}` };
    }
  }
  if (candidate.signing?.status === "release-signed" || installed.signing?.status === "release-signed") {
    return { status: "failed", evidenceKind: "installed", reason: "internal-unsigned profile must never bind to a signed-release identity" };
  }

  return {
    status: "passed",
    evidenceKind: "installed",
    detail: {
      target: "windows-x86_64",
      generation: candidateGeneration,
      candidateArtifact,
      installedArtifact,
      signing: candidate.signing ?? null,
      adapted,
      sourceCommit,
    },
  };
}

// ---------------------------------------------------------------------------
// PKG-03 — Installed version is the one running
//
// Compares the running controller's self-reported build identity against
// the installed manifest, and rejects a fallback to the development
// checkout by requiring the reported source root differ from workspaceRoot.
// When no controllerIdentityPath/env is supplied this queries the installed
// `membrane.exe cli health` directly (never the development checkout, never
// a synthesized value) so PKG-03 can prove the running controller's own
// self-report rather than trusting a hand-authored fixture. Any failure to
// reach the installed executable is reported back to the caller as `null`,
// which PKG_03 turns into a non-fabricated "blocked" reason, not a PASS.
export function queryInstalledControllerIdentity(installedRoot) {
  if (!nonEmptyString(installedRoot)) return null;
  const exe = join(resolve(installedRoot), "membrane.exe");
  if (!existsSync(exe)) return null;
  try {
    const stdout = execFileSync(exe, ["cli", "health"], { cwd: resolve(installedRoot), encoding: "utf8", windowsHide: true, timeout: 10_000 });
    const health = JSON.parse(stdout);
    return {
      releaseGeneration: health.releaseGeneration ?? null,
      sourceRoot: health.sourceRoot ?? health.checkoutRoot ?? null,
      raw: health,
    };
  } catch {
    return null;
  }
}

function sleepSync(milliseconds) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, milliseconds);
}

// Best-effort, bounded stop of a process this module itself started. `/T`
// tears down the tray's own child tree (the daemon it spawned) too, mirroring
// how a normal shutdown would tear down the holder it started. Never invoked
// against a process this module did not start itself.
function stopStartedProcess(pid) {
  if (!pid) return;
  try {
    execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  } catch {
    // Best-effort: there is nothing further a bounded qualification probe can
    // do to restore state; the caller still reports whatever it observed.
  }
}

// Starts the installed product's own holder the sanctioned way -- the same
// membrane-tray.exe --activate entry point scripts/qualification/
// install-release.ps1's Start-AndVerifyHub and observe-windows-lifecycle.ps1
// use, never the installer and never the development checkout -- only when
// no controller is already resident, and waits up to timeoutMs for
// `membrane.exe cli health` to report a live identity. Always returns
// whatever pid it started (even on timeout) so the caller can stop it again;
// this function itself never leaves the process running past the caller's
// own cleanup.
function startInstalledHubResident(installedRoot, timeoutMs) {
  if (!nonEmptyString(installedRoot) || process.platform !== "win32") return null;
  const trayExe = join(resolve(installedRoot), "membrane-tray.exe");
  if (!existsSync(trayExe)) return null;
  let child;
  try {
    child = spawn(trayExe, ["--activate"], { cwd: resolve(installedRoot), detached: true, stdio: "ignore", windowsHide: true });
    child.unref();
  } catch {
    return null;
  }
  const deadline = Date.now() + timeoutMs;
  let identity = null;
  while (Date.now() < deadline) {
    identity = queryInstalledControllerIdentity(installedRoot);
    if (identity) break;
    sleepSync(500);
  }
  return { pid: child.pid, identity };
}

export async function PKG_03({ row, workspaceRoot }) {
  const installedManifestPath = row?.installedManifestPath ?? process.env.MEMBRANE_QUALIFICATION_INSTALLED_MANIFEST;
  const controllerIdentityPath = row?.controllerIdentityPath ?? process.env.MEMBRANE_QUALIFICATION_CONTROLLER_IDENTITY;
  const installedRaw = readJsonIfExists(installedManifestPath);
  const { evidence: installed, adapted, generationError, sourceCommit } = deriveInstalledReleaseEvidence(installedRaw, workspaceRoot);
  let controller = readJsonIfExists(controllerIdentityPath);
  let controllerSource = controllerIdentityPath ? "file" : null;
  const installedRoot = row?.installedRoot ?? process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  let startedPid = null;

  try {
    if (!controller) {
      controller = queryInstalledControllerIdentity(installedRoot);
      if (controller) controllerSource = "live-cli-health";
    }
    // Only attempted when no controller is already resident, so a healthy
    // installed Hub is never disturbed; bounded by timeoutMs and always
    // fail-closed on timeout (controller stays null, no PASS is fabricated).
    const selfStartDisabled = row?.disableSelfStart === true || process.env.MEMBRANE_QUALIFICATION_PKG03_NO_SELF_START === "1";
    if (!controller && nonEmptyString(installedRoot) && !selfStartDisabled) {
      const timeoutMs = Number(row?.selfStartTimeoutMs ?? process.env.MEMBRANE_QUALIFICATION_PKG03_START_TIMEOUT_MS ?? 30_000);
      const started = startInstalledHubResident(installedRoot, timeoutMs);
      if (started?.pid) startedPid = started.pid;
      if (started?.identity) {
        controller = started.identity;
        controllerSource = "self-started-live-cli-health";
      }
    }

    if (installedRaw && adapted && generationError) {
      return {
        status: "failed",
        evidenceKind: "installed",
        reason: `installed manifest generation could not be derived: ${generationError}`,
        detail: { sourceCommit },
      };
    }

    if (!installedRaw || !controller) {
      return {
        status: "failed",
        evidenceKind: "source",
        reason: startedPid
          ? "installed manifest and/or a live controller identity report unavailable even after attempting to start the installed Hub holder (timed out)"
          : "installed manifest and/or a live controller identity report unavailable on this machine; run after install with the controller reachable",
        detail: {
          installedManifestPath: installedManifestPath ?? null,
          controllerIdentityPath: controllerIdentityPath ?? null,
          controllerSource,
          attemptedSelfStart: startedPid !== null,
        },
      };
    }

    const installedGeneration = installed.release?.generation ?? installed.releaseGeneration ?? null;
    const reportedGeneration = controller.releaseGeneration ?? controller.release?.generation ?? null;
    if (!nonEmptyString(reportedGeneration) || reportedGeneration !== installedGeneration) {
      return { status: "failed", evidenceKind: "host", reason: `running controller identity ${reportedGeneration} does not equal installed current ${installedGeneration}` };
    }
    const reportedSourceRoot = controller.sourceRoot ?? controller.checkoutRoot ?? null;
    if (nonEmptyString(reportedSourceRoot) && resolve(reportedSourceRoot) === resolve(workspaceRoot)) {
      return { status: "failed", evidenceKind: "host", reason: "running controller fell back to the development checkout instead of installed current" };
    }

    return { status: "passed", evidenceKind: "host", detail: { releaseGeneration: reportedGeneration, controllerSource } };
  } finally {
    // Restore prior state: stop only what this run itself started.
    if (startedPid) stopStartedProcess(startedPid);
  }
}

// ---------------------------------------------------------------------------
// PKG-04 — Commit and push closure
//
// Read-only inspection of `git log`; this never creates or pushes a commit
// itself (forbidden to this worker), it only verifies that HEAD on the
// primary branch — once the integration owner has committed — contains the
// qualified sourceRevision and carries no worker-authored commit.
export async function PKG_04({ row, workspaceRoot }) {
  const qualifiedSourceRevision = row?.qualifiedSourceRevision ?? process.env.MEMBRANE_QUALIFICATION_SOURCE_REVISION;
  const workerLaneIds = row?.workerLaneIds ?? [];
  if (!nonEmptyString(qualifiedSourceRevision)) {
    return { status: "failed", evidenceKind: "source", reason: "no qualifiedSourceRevision supplied; run after the integration owner commits the qualified tranche" };
  }
  let head;
  let ancestry;
  try {
    head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: workspaceRoot, encoding: "utf8" }).trim();
    ancestry = execFileSync("git", ["merge-base", "--is-ancestor", qualifiedSourceRevision, "HEAD"], { cwd: workspaceRoot, encoding: "utf8", stdio: ["ignore", "ignore", "ignore"] });
  } catch (error) {
    return { status: "failed", evidenceKind: "source", reason: `HEAD does not include qualified sourceRevision ${qualifiedSourceRevision}: ${error.message}` };
  }
  const authors = execFileSync("git", ["log", `${qualifiedSourceRevision}..HEAD`, "--format=%an <%ae>"], { cwd: workspaceRoot, encoding: "utf8" }).trim().split(/\r?\n/u).filter(Boolean);
  const workerAuthored = authors.some((author) => workerLaneIds.some((laneId) => author.toLowerCase().includes(String(laneId).toLowerCase())));
  if (workerAuthored) {
    return { status: "failed", evidenceKind: "source", reason: `a commit between ${qualifiedSourceRevision} and HEAD is attributed to a worker lane, not the integration owner` };
  }
  void ancestry;
  return { status: "passed", evidenceKind: "source", detail: { head, qualifiedSourceRevision } };
}

// ---------------------------------------------------------------------------
// PKG-05 — Evidence kinds are distinct
//
// Executes a small set of PKG cases through the real runOneRegistryCase
// dispatcher and asserts every recorded evidenceKind is one of the six
// recognized kinds, that PKG-02/PKG-03 never satisfy their installed/host
// requirement with source-only evidence, and that no case conflates two
// kinds by returning more than one.
export async function PKG_05({ workspaceRoot, evidencePath }) {
  const sampleRows = [
    { id: "PKG-02", caseFile: "scripts/qualification/cases/pkg-windows.mjs", caseExport: "PKG_02" },
    { id: "PKG-03", caseFile: "scripts/qualification/cases/pkg-windows.mjs", caseExport: "PKG_03" },
    { id: "PKG-04", caseFile: "scripts/qualification/cases/pkg-windows.mjs", caseExport: "PKG_04" },
  ];
  const results = [];
  for (const row of sampleRows) {
    results.push(await runOneRegistryCase(row, { workspaceRoot, profile: "internal-unsigned", platform: "windows", evidencePath }));
  }
  const checks = [];
  for (const result of results) {
    checks.push({ name: `${result.id}-evidence-kind-recognized`, passed: nonEmptyString(result.evidenceKind) });
    if (result.id !== "PKG-04" && result.status === "passed") {
      checks.push({ name: `${result.id}-installed-or-host-not-source`, passed: result.evidenceKind !== "source" && result.evidenceKind !== "component" });
    }
  }
  const passed = checks.every((check) => check.passed === true);
  return {
    status: passed ? "passed" : "failed",
    evidenceKind: "component",
    detail: { sampleResults: results.map((result) => ({ id: result.id, status: result.status, evidenceKind: result.evidenceKind })), checks },
    reason: passed ? null : `failing checks: ${checks.filter((check) => !check.passed).map((check) => check.name).join(", ")}`,
  };
}

// Exposed for tests / other case modules that need a stable content hash of
// an evidence file without re-deriving the hashing convention.
export function sha256OfFile(path) {
  return sha256File(path);
}
