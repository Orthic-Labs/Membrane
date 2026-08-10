import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const MANIFEST_SCHEMA_VERSION = 1;
const PRODUCT_ID = "cortex";

export function buildProductManifest({ installRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../.."), version = null } = {}) {
  const pkg = JSON.parse(readFileSync(join(installRoot, "package.json"), "utf8"));
  const ver = version ?? pkg.version ?? "0.0.0";
  const root = resolve(installRoot);
  // D-S03: Hub spawns as child — serviceStart is foreground argv
  const serviceStart = [process.execPath, join(root, "scripts", "cortex.mjs"), "service", "run"];
  const serviceStop = [process.execPath, join(root, "scripts", "cortex.mjs"), "service", "stop"];
  // statusEndpoint — loopback, will be filled by snapshot server at runtime; placeholder loopback address
  const statusEndpoint = { host: "127.0.0.1", port: 0, tokenEnv: "CORTEX_SNAPSHOT_TOKEN" };
  const icon = join(root, "assets", "icon", "cortex-tab.png");
  return {
    schemaVersion: MANIFEST_SCHEMA_VERSION,
    productId: PRODUCT_ID,
    displayName: "Cortex",
    version: ver,
    installRoot: root,
    serviceStart,
    serviceStop,
    statusEndpoint,
    icon,
  };
}

export function manifestPath() {
  return join(homedir(), ".orthic", "hub", "products.d", "cortex.json");
}

export function writeProductManifest({ installRoot, version, outPath = manifestPath() } = {}) {
  const manifest = buildProductManifest({ installRoot, version });
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, JSON.stringify(manifest, null, 2) + "\n", "utf8");
  return { path: outPath, manifest };
}

export function validateProductManifest(manifest) {
  const errors = [];
  if (manifest.schemaVersion !== 1) errors.push("schemaVersion must be 1");
  if (manifest.productId !== "cortex") errors.push("productId must be cortex");
  if (!manifest.installRoot || !existsSync(manifest.installRoot)) errors.push("installRoot must exist");
  if (!Array.isArray(manifest.serviceStart) || !manifest.serviceStart.length) errors.push("serviceStart required");
  if (!Array.isArray(manifest.serviceStop) || !manifest.serviceStop.length) errors.push("serviceStop required");
  if (!manifest.icon || !manifest.icon.startsWith(manifest.installRoot)) errors.push("icon must be inside installRoot");
  // Hub security: resolve and check inside installRoot after symlink resolution
  try {
    const realIcon = realpathSync(manifest.icon);
    const realRoot = realpathSync(manifest.installRoot);
    if (!realIcon.startsWith(realRoot)) errors.push("icon symlink escapes installRoot");
  } catch {}
  return { ok: errors.length === 0, errors };
}
