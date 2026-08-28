import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

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
  if (process.env.MEMBRANE_SIGNED_SIDECARS_READY !== "1") {
    throw new Error("signed Windows sidecars are not prepared");
  }
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
  if (!existsSync(signedRaw)) throw new Error(`signed raw Hub executable is missing: ${signedRaw}`);
  const temporaryRoot = mkdtempSync(join(tmpdir(), "membrane-hub-release-"));
  const signedBackup = join(temporaryRoot, rawRelative);
  cpSync(signedRaw, signedBackup);
  try {
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
}
