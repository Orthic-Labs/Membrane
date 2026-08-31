import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";
import { verifyNsisEmbeddedBinary } from "@rightkit/release/nsis-payload.mjs";

if (process.platform !== "win32") throw new Error("Windows installer must bundle on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const output = join(hub, "dist", "portable");
const pkg = JSON.parse(readFileSync(join(hub, "package.json"), "utf8"));
const payload = join(output, `membrane-${pkg.version}-windows-x86_64`);
const embedded = join(hub, "src-tauri", "installer-release");
const bundleConfig = join(output, "tauri.release-bundle.conf.json");
const target = "x86_64-pc-windows-msvc";
const managedRelease = join(resolveTargetRoot(join(hub, "src-tauri", "Cargo.toml")), target, "release");
const manifest = JSON.parse(readFileSync(join(output, "release-manifest.json"), "utf8"));
const asset = manifest.assets?.find((entry) => entry.target === "windows-x86_64");
if (!asset) throw new Error("finalized direct release has no Windows asset");

function sha256(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }
function requireFile(path, label) { if (!existsSync(path)) throw new Error(`${label} is missing: ${path}`); return path; }
function run(command, args, env = process.env) {
  const executable = command === "pnpm" ? "pnpm.cmd" : command;
  const result = spawnSync(executable, args, { cwd: hub, env, stdio: "inherit", shell: executable.endsWith(".cmd"), windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} exited ${result.status}`);
}

const releaseFiles = [
  "release-manifest.json",
  "release-manifest.cat",
  "checksums.json",
  "install.ps1",
  asset.name,
  asset.provenanceName,
  asset.sbomName,
];
rmSync(embedded, { recursive: true, force: true });
mkdirSync(embedded, { recursive: true });
try {
  for (const name of releaseFiles) cpSync(requireFile(join(output, name), `direct-release ${name}`), join(embedded, name));

  const executables = [
    ["membrane-hub.exe", join(managedRelease, "membrane-hub.exe")],
    ["cortex.exe", join(hub, "src-tauri", "binaries", `cortex-${target}.exe`)],
    ["membrane.exe", join(hub, "src-tauri", "binaries", `membrane-${target}.exe`)],
    ["membrane-tray.exe", join(hub, "src-tauri", "binaries", `membrane-tray-${target}.exe`)],
    ["membrane-daemon.exe", join(hub, "src-tauri", "binaries", `membrane-daemon-${target}.exe`)],
  ];
  for (const [name, destination] of executables) {
    mkdirSync(dirname(destination), { recursive: true });
    cpSync(requireFile(join(payload, name), `signed payload ${name}`), destination);
  }

  const config = JSON.stringify({ bundle: {
    createUpdaterArtifacts: false,
    externalBin: ["binaries/cortex", "binaries/membrane", "binaries/membrane-tray", "binaries/membrane-daemon"],
    resources: ["installer-release"],
    windows: { allowDowngrades: true, nsis: { template: "windows/installer.nsi" } },
  } });
  writeFileSync(bundleConfig, `${config}\n`);
  run("pnpm", ["exec", "tauri", "bundle", "--target", target, "--bundles", "nsis", "--config", bundleConfig], {
    ...process.env,
    MEMBRANE_SIDECARS_READY: "1",
    MEMBRANE_SIGNED_SIDECARS_READY: "1",
    TAURI_ENV_TARGET_TRIPLE: target,
  });
  const generated = requireFile(join(managedRelease, "bundle", "nsis", `Membrane Hub_${pkg.version}_x64-setup.exe`), "generated NSIS installer");
  const installer = join(output, `Membrane_Hub_${pkg.version}_x64-setup.exe`);
  cpSync(generated, installer);

  const receipts = releaseFiles.map((name) => verifyNsisEmbeddedBinary({ installer, entryName: basename(name), expectedSha256: sha256(join(output, name)) }));
  const receipt = { schemaVersion: 1, contract: "membrane-nsis-direct-release-embedding-v1", installer: basename(installer), installerSha256: sha256(installer), embedded: receipts };
  writeFileSync(join(output, "nsis-embedded-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(JSON.stringify(receipt));
} finally {
  rmSync(embedded, { recursive: true, force: true });
  rmSync(bundleConfig, { force: true });
}
