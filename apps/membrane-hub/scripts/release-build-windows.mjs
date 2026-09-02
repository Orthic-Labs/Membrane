import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

if (process.platform !== "win32") throw new Error("Windows release build must run on Windows");

const sidecars = [
  "src-tauri/binaries/cortex-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-tray-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-daemon-x86_64-pc-windows-msvc.exe",
];

function prepareNativeBinaries() {
  // `pnpm run build` writes current release identity before compiling & staging
  // cortex/membrane. Daemon/tray then compile against that same identity.
  run(["run", "build"]);
  cargo(["build", "--manifest-path", "../../engine/Cargo.toml", "--release", "--target", "x86_64-pc-windows-msvc", "-p", "membrane-runtime", "--bin", "membrane-daemon"]);
  cargo(["build", "--manifest-path", "../membrane-tray-windows/Cargo.toml", "--release", "--target", "x86_64-pc-windows-msvc"]);
  const hub = fileURLToPath(new URL("../", import.meta.url));
  const target = "x86_64-pc-windows-msvc";
  const engineRelease = join(resolveTargetRoot(join(hub, "../../engine/Cargo.toml")), target, "release");
  const outputs = [
    [join(engineRelease, "cortex.exe"), join(hub, "src-tauri/binaries/cortex-x86_64-pc-windows-msvc.exe")],
    [join(engineRelease, "membrane.exe"), join(hub, "src-tauri/binaries/membrane-x86_64-pc-windows-msvc.exe")],
    [join(engineRelease, "membrane-daemon.exe"), join(hub, "src-tauri/binaries/membrane-daemon-x86_64-pc-windows-msvc.exe")],
    [join(resolveTargetRoot(join(hub, "../membrane-tray-windows/Cargo.toml")), target, "release", "membrane-tray-windows.exe"), join(hub, "src-tauri/binaries/membrane-tray-x86_64-pc-windows-msvc.exe")],
  ];
  mkdirSync(join(hub, "src-tauri/binaries"), { recursive: true });
  for (const [source, destination] of outputs) {
    if (!existsSync(source)) throw new Error(`native Windows artifact missing: ${source}`);
    cpSync(source, destination);
  }
}

// The workspace routes Cargo through RightKit; public CI has no RightKit and
// compiles directly, which is what MEMBRANE_PUBLIC_CI_DIRECT_CARGO already
// signals for the release candidate. Honour it here too, so the unsigned build
// works on a hosted runner (run 33682896568 failed with `rightkit` not found).
function cargo(args) {
  if (process.env.MEMBRANE_PUBLIC_CI_DIRECT_CARGO === "1") {
    const result = spawnSync("cargo", args, {
      cwd: new URL("../", import.meta.url),
      encoding: "utf8",
      shell: true,
      stdio: "inherit",
      windowsHide: true,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`cargo exited ${result.status}`);
    return;
  }
  run(["exec", "rightkit", "cargo", ...args]);
}

function run(args, env = process.env) {
  const result = spawnSync("pnpm.cmd", args, {
    cwd: new URL("../", import.meta.url),
    encoding: "utf8",
    env,
    shell: true,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`pnpm exited ${result.status}`);
}

if (process.argv.includes("--prepare-only")) {
  prepareNativeBinaries();
  process.exit(0);
}

// An installable build with no certificate anywhere in the path: the same
// sidecars, the same Tauri build, the same NSIS script, no Authenticode and no
// release chain. This is the loop for testing a change on a real desktop;
// signing and publication stay a separate, later concern.
if (process.argv.includes("--unsigned")) {
  prepareNativeBinaries();
  const unsigned = { ...process.env, MEMBRANE_UNSIGNED_INSTALLER: "1" };
  run(["exec", "node", "scripts/build-windows-release.mjs", "raw"], unsigned);
  run(["exec", "node", "scripts/build-windows-release.mjs", "package"], unsigned);
  process.exit(0);
}
prepareNativeBinaries();
run(["exec", "right-release", "sign-windows", ...sidecars]);
run(["exec", "right-release", "sign-windows", "--verify-only", ...sidecars]);
run(["exec", "right-release", "build", "--platform", "win"], {
  ...process.env,
  MEMBRANE_SIGNED_SIDECARS_READY: "1",
});
