import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative } from "node:path";
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
function filesUnder(root) {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const full = join(root, entry.name);
    return entry.isDirectory() ? filesUnder(full) : [full];
  });
}
function run(command, args, env = process.env) {
  const executable = command === "pnpm" ? "pnpm.cmd" : command;
  const result = spawnSync(executable, args, { cwd: hub, env, stdio: "inherit", shell: executable.endsWith(".cmd"), windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} exited ${result.status}`);
}

const metadataFiles = [
  "release-manifest.json",
  "release-manifest.cat",
  "checksums.json",
  asset.provenanceName,
  asset.sbomName,
];
const versionDirRelative = `versions/${pkg.version}`;
rmSync(embedded, { recursive: true, force: true });
mkdirSync(embedded, { recursive: true });
try {
  for (const name of metadataFiles) cpSync(requireFile(join(output, name), `direct-release ${name}`), join(embedded, name));

  // Extract the signed portable archive into installer-release/versions/<v>/ so
  // NSIS embeds the whole version tree as its own File payload. Section Install
  // then lays it down with a plain recursive copy and no scripting host at all;
  // the zip itself no longer needs embedding. `tar` ships on Windows 10+ and the
  // GitHub runners.
  const stagedVersion = join(embedded, "versions", pkg.version);
  mkdirSync(stagedVersion, { recursive: true });
  const archivePath = requireFile(join(output, asset.name), `direct-release ${asset.name}`);
  const extraction = spawnSync("tar", ["-xf", archivePath, "-C", stagedVersion], { stdio: "inherit", windowsHide: true });
  if (extraction.error) throw extraction.error;
  if (extraction.status !== 0) throw new Error(`tar -xf ${asset.name} exited ${extraction.status}`);

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

  const versionTreeFiles = filesUnder(stagedVersion).map((absolute) => relative(embedded, absolute).replaceAll("\\", "/"));
  const resources = {};
  for (const name of metadataFiles) resources[`installer-release/${name}`] = name;
  for (const relativePath of versionTreeFiles) resources[`installer-release/${relativePath}`] = relativePath;
  // Section Install now lays versions/<v> and activates directly. A generated
  // RightRelease install.ps1 must never re-enter the Membrane NSIS payload.
  for (const name of Object.values(resources)) {
    if (/(^|[\/])install\.ps1$/i.test(String(name))) {
      throw new Error(`Membrane NSIS payload must not embed a generated bootstrap: ${name}`);
    }
  }
  const config = JSON.stringify({ bundle: {
    createUpdaterArtifacts: false,
    externalBin: ["binaries/cortex", "binaries/membrane", "binaries/membrane-tray", "binaries/membrane-daemon"],
    resources,
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

  const receiptEntries = [
    { entryName: "release-manifest.json", diskPath: join(output, "release-manifest.json") },
    { entryName: "release-manifest.cat", diskPath: join(output, "release-manifest.cat") },
    { entryName: "checksums.json", diskPath: join(output, "checksums.json") },
    ...["membrane.exe", "membrane-tray.exe", "membrane-daemon.exe", "membrane-hub.exe", "cortex.exe"].map((name) => ({
      entryName: `${versionDirRelative}/${name}`,
      diskPath: join(stagedVersion, name),
    })),
  ];
  const receipts = receiptEntries.map(({ entryName, diskPath }) => ({
    ...verifyNsisEmbeddedBinary({ installer, entryName, expectedSha256: sha256(requireFile(diskPath, `embedded ${entryName}`)) }),
    entry: entryName,
  }));
  const receipt = { schemaVersion: 1, contract: "membrane-nsis-direct-release-embedding-v1", installer: basename(installer), installerSha256: sha256(installer), embedded: receipts };
  writeFileSync(join(output, "nsis-embedded-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(JSON.stringify(receipt));
} finally {
  rmSync(embedded, { recursive: true, force: true });
  rmSync(bundleConfig, { force: true });
}
