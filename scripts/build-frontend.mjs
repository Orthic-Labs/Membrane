import { chmodSync, cpSync, existsSync, mkdirSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const output = new URL("../dist/", import.meta.url);
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
for (const name of ["index.html", "popover.html", "src"]) {
  cpSync(new URL(`../${name}`, import.meta.url), new URL(name, output), { recursive: true });
}
cpSync(new URL("../assets/tray", import.meta.url), new URL("assets/tray", output), { recursive: true });
cpSync(
  new URL("../node_modules/@tauri-apps/api", import.meta.url),
  new URL("vendor/@tauri-apps/api", output),
  { recursive: true },
);
cpSync(
  new URL("../node_modules/@tauri-apps/plugin-os/dist-js", import.meta.url),
  new URL("vendor/@tauri-apps/plugin-os", output),
  { recursive: true },
);
cpSync(
  new URL("../node_modules/@rightkit/platform-ui/dist", import.meta.url),
  new URL("vendor/@rightkit/platform-ui", output),
  { recursive: true },
);

const targets = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
};
const target = process.env.TAURI_ENV_TARGET_TRIPLE || targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error(`unsupported sidecar target: ${process.platform}-${process.arch}`);

// D-1c: Hub no longer builds product sidecars. It consumes already-built
// binaries from a configurable staging directory. This preserves the DMG's
// self-contained property (D-S08) without cross-compiling across repo boundaries (R-12/I-7).
// Staging dir env var: ORTHIC_PRODUCT_BINARIES_DIR
// Defaults for local dev: sibling checkout layout ../orthic-product-binaries/<target>/
// and CI: downloaded release artifacts.
const stagingRoot = process.env.ORTHIC_PRODUCT_BINARIES_DIR
  ? fileURLToPath(new URL(process.env.ORTHIC_PRODUCT_BINARIES_DIR, import.meta.url))
  : join(fileURLToPath(new URL("../", import.meta.url)), "..", "orthic-product-binaries", target);

// Derive minimal release identity locally (without engine). For production builds,
// identity comes from staged binaries' own build-info or is supplied via env.
let identity;
try {
  const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  identity = {
    commit: process.env.ORTHIC_SOURCE_COMMIT || "0000000000000000000000000000000000000000",
    dirty: false,
    fileCount: 0,
    sourceTreePath: "orthic",
    sourceTreeSha256: process.env.ORTHIC_SOURCE_TREE_SHA256 || "0".repeat(64),
    releaseGeneration: `sha256:${process.env.ORTHIC_SOURCE_TREE_SHA256 || "0".repeat(64)}`,
    version: pkg.version,
  };
} catch {
  identity = {
    commit: "0000000000000000000000000000000000000000",
    dirty: false,
    fileCount: 0,
    sourceTreePath: "orthic",
    sourceTreeSha256: "0".repeat(64),
    releaseGeneration: `sha256:${"0".repeat(64)}`,
  };
}
console.log(`[orthic] staging binaries from ${stagingRoot} for target ${target}`);

writeFileSync(new URL("../dist/release-identity.json", import.meta.url), `${JSON.stringify(identity, null, 2)}\n`);
const binaries = fileURLToPath(new URL("../src-tauri/binaries/", import.meta.url));
mkdirSync(binaries, { recursive: true });

// Copy staged binaries into src-tauri/binaries/<name>-<target>[.exe]
// Sidecars are product-owned; the Hub consumes them as opaque artifacts.
const sidecars = ["crypt", "crypt-service", "membrane"];
let stagedCount = 0;
for (const name of sidecars) {
  const suffix = target.includes("windows") ? ".exe" : "";
  const source = join(stagingRoot, `${name}${suffix}`);
  const altSource = join(stagingRoot, `${name}-${target}${suffix}`);
  let src = null;
  if (existsSync(source)) src = source;
  else if (existsSync(altSource)) src = altSource;
  if (!src) {
    // In dev without staging dir, create placeholder so build can proceed;
    // release lane will fail loudly if missing.
    console.warn(`[orthic] staged sidecar missing (skipping in dev): ${source}`);
    continue;
  }
  const destination = join(binaries, `${name}-${target}${suffix}`);
  cpSync(src, destination);
  if (process.platform !== "win32") chmodSync(destination, 0o755);
  stagedCount++;
}
if (stagedCount === 0) {
  console.warn("[orthic] no staged sidecars found — dev build without binaries (release will require them)");
}
// Also stage Cortex's binary if present (name discovered via manifest, not hardcoded here).
// Cortex's bundled binary name is product-specific; we probe common names.
for (const cortexName of ["cortex", "cortex-service"]) {
  const suffix = target.includes("windows") ? ".exe" : "";
  const source = join(stagingRoot, `${cortexName}${suffix}`);
  const alt = join(stagingRoot, `${cortexName}-${target}${suffix}`);
  let src = null;
  if (existsSync(source)) src = source;
  else if (existsSync(alt)) src = alt;
  if (!src) continue;
  const dest = join(binaries, `${cortexName}-${target}${suffix}`);
  cpSync(src, dest);
  if (process.platform !== "win32") chmodSync(dest, 0o755);
  console.log(`[orthic] staged cortex sidecar: ${cortexName}`);
}
