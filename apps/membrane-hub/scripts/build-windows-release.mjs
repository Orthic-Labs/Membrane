import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

// The local unsigned route (MEMBRANE_UNSIGNED_INSTALLER=1) never runs on
// GitHub Actions, so it can never produce the CI-only candidate that
// release-build-candidate-windows.mjs emits under RIGHT_GIT_ARTIFACT_ROOT.
// It still needs a comparable, non-fabricated candidate.json next to the
// installer it built, so a local qualification run can bind package
// identity against the installed manifest (PKG-02) without waiting on CI.
// This reuses the exact release-identity input the sidecars were compiled
// against (dist/release-identity.json, written by build-frontend.mjs via
// writeEngineReleaseIdentity) rather than recomputing it, so the reported
// generation always matches the bytes that were actually built.
export function writeUnsignedCandidateManifest({ hubRoot, installerPath, version, outputPath }) {
  const identityPath = join(hubRoot, "dist", "release-identity.json");
  if (!existsSync(identityPath)) throw new Error(`release identity missing: ${identityPath}; run pnpm run build first`);
  const identity = JSON.parse(readFileSync(identityPath, "utf8"));
  if (!/^[0-9a-f]{64}$/.test(identity.sourceTreeSha256) || identity.releaseGeneration !== `sha256:${identity.sourceTreeSha256}`) {
    throw new Error("release identity is not hash-bound");
  }
  if (!existsSync(installerPath)) throw new Error(`installer artifact missing: ${installerPath}`);
  const artifactSha256 = createHash("sha256").update(readFileSync(installerPath)).digest("hex");
  const artifact = { path: basename(installerPath), sha256: artifactSha256, size: statSync(installerPath).size };
  const candidate = {
    schema: "membrane.release-evidence.v1",
    product: "Membrane Hub",
    profile: "internal-unsigned",
    sourceCommit: identity.commit,
    dirty: identity.dirty,
    release: {
      tag: `v${version}`,
      version,
      commit: identity.commit,
      tree: identity.sourceTreeSha256,
      generation: identity.sourceTreeSha256,
      target: "windows-x86_64",
      artifact_sha256: artifactSha256,
    },
    artifact,
    signing: { status: "unsigned", reason: "internal_local_unsigned_route" },
  };
  const sbom = {
    schema: "membrane.sbom.v1",
    signing: candidate.signing,
    artifact,
    package: { name: "membrane-hub", version, target: "windows-x86_64" },
    components: [{ name: "Membrane Hub Windows installer", type: "application", sha256: artifactSha256 }],
  };
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(candidate, null, 2)}\n`);
  writeFileSync(join(dirname(outputPath), "sbom.json"), `${JSON.stringify(sbom, null, 2)}\n`);
  return candidate;
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (invokedDirectly) runCli();

function runCli() {
if (process.platform !== "win32") throw new Error("Windows package must run on Windows");

const phase = process.argv.slice(2).find((argument) => argument !== "--");
if (!new Set(["raw", "package"]).has(phase)) {
  throw new Error("usage: build-windows-release.mjs <raw|package>");
}

const hubRoot = fileURLToPath(new URL("../", import.meta.url));
const triple = "x86_64-pc-windows-msvc";
const packageJson = JSON.parse(readFileSync(join(hubRoot, "package.json"), "utf8"));
const managedRelease = join(resolveTargetRoot(join(hubRoot, "src-tauri", "Cargo.toml")), triple, "release");
const sealedRelease = join(hubRoot, "src-tauri", "target", triple, "release");
const rawRelative = "membrane-hub.exe";
const generatedInstallerRelative = join("bundle", "nsis", `Membrane Hub_${packageJson.version}_x64-setup.exe`);
const installerRelative = join("bundle", "nsis", `Membrane_Hub_${packageJson.version}_x64-setup.exe`);

function mirror(source, destination, label) {
  if (!existsSync(source)) throw new Error(`${label} is missing: ${source}`);
  const sourcePath = realpathSync.native(source).toLowerCase();
  const destinationPath = existsSync(destination)
    ? realpathSync.native(destination).toLowerCase()
    : resolve(destination).toLowerCase();
  if (sourcePath === destinationPath) return;
  mkdirSync(dirname(destination), { recursive: true });
  cpSync(source, destination);
}

function run(command, args, { sidecarsReady = false } = {}) {
  const env = { ...process.env, TAURI_ENV_TARGET_TRIPLE: "x86_64-pc-windows-msvc" };
  if (sidecarsReady) env.MEMBRANE_SIDECARS_READY = "1";
  else delete env.MEMBRANE_SIDECARS_READY;
  const executable = command === "pnpm" ? "pnpm.cmd" : command;
  const result = spawnSync(executable, args, {
    cwd: new URL("../", import.meta.url),
    env,
    shell: executable.endsWith(".cmd"),
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} exited ${result.status}`);
}

if (phase === "raw") {
  // RightKit patches & signs this raw EXE before invoking this script's package
  // phase.  Bundling at this point would let Tauri mutate a signed executable.
  // Sidecars are built, signed, & verified locally before RightKit starts its
  // raw/package contract. The hook never recurses into right-release.
  // Signing is a release concern, not a build one: NSIS needs no certificate.
  // MEMBRANE_UNSIGNED_INSTALLER=1 builds the same installer from unsigned
  // sidecars so a change can be installed and tested on a real desktop without
  // any certificate authority in the loop. Release builds never set it.
  if (process.env.MEMBRANE_UNSIGNED_INSTALLER !== "1" && process.env.MEMBRANE_SIGNED_SIDECARS_READY !== "1") {
    throw new Error("signed Windows sidecars are not prepared");
  }
  // Tauri resolves bundle resources on every invocation, so the versions
  // directory has to exist even in the raw phase, which runs before there is
  // a Hub executable to stage into it.
  mkdirSync(join(hubRoot, "src-tauri", "versions", packageJson.version), { recursive: true });
  run("pnpm", ["run", "build"], { sidecarsReady: true });
  run("node", ["scripts/stage-runtime.mjs"], { sidecarsReady: true });
  run("pnpm", ["exec", "tauri", "build", "--target", triple, "--no-bundle", "--config", "src-tauri/tauri.windows.conf.json"], { sidecarsReady: true });
  mirror(join(managedRelease, rawRelative), join(sealedRelease, rawRelative), "managed raw Hub executable");
} else {
  // right-release signed the mirrored raw EXE between phases. Put those exact
  // managed-target bytes back before NSIS embeds them. Tauri's bundle
  // preparation strips Authenticode while generating installer inputs, so
  // preserve signed bytes, restore them, then rerun only deterministic NSIS.
  const signedRaw = join(managedRelease, rawRelative);
  if (!existsSync(signedRaw)) {
    throw new Error(
      process.env.MEMBRANE_UNSIGNED_INSTALLER === "1"
        ? `raw Hub executable is missing: ${signedRaw}`
        : `signed raw Hub executable is missing: ${signedRaw}`,
    );
  }
  const temporaryRoot = mkdtempSync(join(tmpdir(), "membrane-hub-release-"));
  const signedBackup = join(temporaryRoot, rawRelative);
  cpSync(signedRaw, signedBackup);
  try {
    // Stage the versions/<version> tree the installer must lay down. Until
    // this existed the NSIS payload carried only `runtime`, so a "successful"
    // install placed no executable at all and verify-version-tree passed
    // solely on files an earlier install had left behind.
    const versionTree = join(hubRoot, "src-tauri", "versions", packageJson.version);
    rmSync(join(hubRoot, "src-tauri", "versions"), { recursive: true, force: true });
    run("node", [
      "scripts/package-portable-windows.mjs",
      "--hub-exe", signedRaw,
      "--started-at", new Date().toISOString(),
      "--payload-dir", versionTree,
      "--payload-only",
    ], { sidecarsReady: true });
    run("pnpm", ["exec", "tauri", "bundle", "--target", triple, "--bundles", "nsis", "--config", "src-tauri/tauri.windows.conf.json"], { sidecarsReady: true });
    mirror(signedBackup, join(managedRelease, rawRelative), "preserved signed raw Hub executable");
    const localAppData = process.env.LOCALAPPDATA;
    if (!localAppData) throw new Error("LOCALAPPDATA is required to locate Tauri NSIS");
    run(join(localAppData, "tauri", "NSIS", "makensis.exe"), [
      "-INPUTCHARSET", "UTF8", "-OUTPUTCHARSET", "UTF8", "-V1", join(managedRelease, "nsis", "x64", "installer.nsi"),
    ]);
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
  mirror(join(managedRelease, generatedInstallerRelative), join(managedRelease, installerRelative), "generated NSIS installer");
  mirror(join(managedRelease, installerRelative), join(sealedRelease, installerRelative), "managed NSIS installer");
  if (process.env.MEMBRANE_UNSIGNED_INSTALLER === "1") {
    // Emit the local-route candidate next to each mirrored copy of the
    // installer so a qualification run pointed at either location (managed
    // RightKit workspace or the sealed src-tauri target) finds it.
    for (const installerPath of [join(managedRelease, installerRelative), join(sealedRelease, installerRelative)]) {
      writeUnsignedCandidateManifest({
        hubRoot,
        installerPath,
        version: packageJson.version,
        outputPath: join(dirname(installerPath), "candidate.json"),
      });
    }
  }
}
}
