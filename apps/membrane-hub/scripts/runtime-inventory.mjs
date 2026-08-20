import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const hub = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(hub, "src-tauri", "runtime");
const required = [
  ["membrane-cli", "src-tauri/binaries/membrane-{target}"],
  ["cortex-service", "src-tauri/binaries/cortex-service-{target}"],
  ["cortex-cli", "src-tauri/binaries/cortex-{target}"],
  ["hub-contract", "../../schemas/operations/membrane-blueprint.v1.schema.json"],
  ["guide", "../../docs/subsystems/guide.md"],
  ["push", "../../docs/subsystems/push.md"],
  ["adapt", "../../adapt/src/adapt/manifest.py"],
  ["blueprint", "../../blueprint/src/service/protocol.mjs"],
  ["license", "../../LICENSE"],
];
const retired = /(?:^|[\\/])(crypt(?:-service)?|orthic(?:[_-]manifest)?|product-addons?)(?:[\\/]|$)/i;
const hash = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

export function inventory() {
  const entries = required.map(([role, source]) => {
    const target = process.env.TAURI_ENV_TARGET_TRIPLE || (process.platform === "win32" ? "x86_64-pc-windows-msvc" : "aarch64-apple-darwin");
    const extension = target.includes("windows") ? ".exe" : "";
    const concrete = source.replace("{target}", `${target}${extension}`);
    const absolute = resolve(hub, concrete);
    if (!existsSync(absolute) || !statSync(absolute).isFile()) throw new Error(`runtime asset missing: ${role} (${source})`);
    if (retired.test(concrete)) throw new Error(`retired runtime asset rejected: ${concrete}`);
    const sourcePath = relative(hub, absolute).replaceAll("\\", "/");
    const delivery = source.startsWith("src-tauri/binaries/") ? "externalBin" : "resource";
    return {
      role,
      source: sourcePath,
      sha256: hash(absolute),
      delivery,
      stagePath: delivery === "externalBin" ? `external-bin/${basename(absolute)}` : `resources/${role}/${basename(absolute)}`,
      installerPath: delivery === "externalBin" ? `sidecars/${basename(absolute)}` : `resources/${role}/${basename(absolute)}`,
    };
  });
  if (new Set(entries.map((entry) => entry.role)).size !== entries.length) throw new Error("duplicate runtime role");
  return { schemaVersion: 1, app: "membrane-hub", composition: ["membrane", "cortex-service", "blueprint", "guide", "pull", "push", "adapt"], entries };
}

export function writeInventory() {
  const value = inventory();
  rmSync(runtime, { recursive: true, force: true });
  mkdirSync(runtime, { recursive: true });
  for (const entry of value.entries.filter((entry) => entry.delivery === "resource")) {
    const destination = join(runtime, entry.stagePath);
    mkdirSync(resolve(destination, ".."), { recursive: true });
    copyFileSync(resolve(hub, entry.source), destination);
    if (hash(destination) !== entry.sha256) throw new Error(`staged runtime asset hash mismatch: ${entry.role}`);
  }
  writeFileSync(join(runtime, "runtime-inventory.json"), `${JSON.stringify(value, null, 2)}\n`);
  return value;
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  const action = process.argv[2] || "check";
  if (action === "write") writeInventory();
  else if (action === "check") inventory();
  else throw new Error("usage: runtime-inventory.mjs <check|write>");
}
