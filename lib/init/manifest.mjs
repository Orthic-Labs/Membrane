import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const MANIFEST_SCHEMA_VERSION = 1;
const PRODUCT_ID = "cortex";
const DEFAULT_SNAPSHOT_PORT = 43127;

function snapshotPort(value = process.env.CORTEX_SNAPSHOT_PORT) {
  const port = Number(value ?? DEFAULT_SNAPSHOT_PORT);
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : DEFAULT_SNAPSHOT_PORT;
}

function snapshotToken(value = process.env.CORTEX_SNAPSHOT_TOKEN) {
  return typeof value === "string" && value.length > 0 ? value : randomBytes(32).toString("base64url");
}

export function buildProductManifest({ installRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../.."), version = null } = {}) {
  const pkg = JSON.parse(readFileSync(join(installRoot, "package.json"), "utf8"));
  const ver = version ?? pkg.version ?? "0.0.0";
  const root = resolve(installRoot);
  // D-S03: Hub spawns as child — serviceStart is foreground argv
  const serviceStart = [process.execPath, join(root, "scripts", "cortex.mjs"), "service", "run", "--root", root];
  const serviceStop = [process.execPath, join(root, "scripts", "cortex.mjs"), "service", "stop"];
  const statusEndpoint = { host: "127.0.0.1", port: snapshotPort(), authHeader: "Authorization", authToken: snapshotToken() };
  const icon = join(root, "assets", "icon", "cortex-tab.png");
  return {
    schemaVersion: MANIFEST_SCHEMA_VERSION,
    productId: PRODUCT_ID,
    displayName: "Cortex",
    productVersion: ver,
    hubCompatRange: ">=0.1.0",
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
  if (!manifest.displayName) errors.push("displayName required");
  if (!manifest.productVersion) errors.push("productVersion required");
  if (!manifest.hubCompatRange) errors.push("hubCompatRange required");
  if (!manifest.installRoot || !existsSync(manifest.installRoot)) errors.push("installRoot must exist");
  if (!Array.isArray(manifest.serviceStart) || !manifest.serviceStart.length) errors.push("serviceStart required");
  if (!Array.isArray(manifest.serviceStop)) errors.push("serviceStop required");
  if (manifest.statusEndpoint?.host !== "127.0.0.1") errors.push("statusEndpoint must be loopback");
  if (!Number.isInteger(manifest.statusEndpoint?.port) || manifest.statusEndpoint.port < 1 || manifest.statusEndpoint.port > 65535) errors.push("statusEndpoint port must be valid");
  if (!manifest.statusEndpoint?.authHeader || !manifest.statusEndpoint?.authToken) errors.push("statusEndpoint auth required");
  if (!manifest.icon || !manifest.icon.startsWith(manifest.installRoot)) errors.push("icon must be inside installRoot");
  // Hub security: resolve and check inside installRoot after symlink resolution
  try {
    const realIcon = realpathSync(manifest.icon);
    const realRoot = realpathSync(manifest.installRoot);
    if (!realIcon.startsWith(`${realRoot}/`) && realIcon !== realRoot) errors.push("icon symlink escapes installRoot");
  } catch {}
  return { ok: errors.length === 0, errors };
}
