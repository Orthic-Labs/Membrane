// Native-host artifact proof consumes only packaged Mac runtime bytes/sidecars.
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { verifyUnpackedArtifact } from "./runtime-inventory.mjs";

function findRuntime(root) {
  const pending = [root];
  while (pending.length) {
    const current = pending.pop();
    if (existsSync(join(current, "resources", "runtime", "runtime-inventory.json"))) return { runtimeDir: join(current, "resources", "runtime"), sidecarDir: current };
    for (const entry of readdirSync(current, { withFileTypes: true })) if (entry.isDirectory()) pending.push(join(current, entry.name));
  }
  throw new Error("unpacked runtime inventory missing");
}
function appLayout(app) {
  const mac = join(app, "Contents", "Resources", "runtime");
  if (existsSync(join(mac, "runtime-inventory.json"))) return { runtimeDir: mac, sidecarDir: join(app, "Contents", "MacOS") };
  return findRuntime(app);
}
function args(argv) {
  const result = {};
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index]; if (!token.startsWith("--")) throw new Error(`unknown argument: ${token}`);
    result[token.slice(2)] = argv[++index]; if (!result[token.slice(2)]) throw new Error(`value required: ${token}`);
  }
  return result;
}

export async function verifyPackagedArtifact({ app } = {}) {
  if (!app) throw new Error("supply --app");
  return verifyUnpackedArtifact(appLayout(resolve(app)));
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  verifyPackagedArtifact(args(process.argv.slice(2))).catch((error) => { console.error(error.message); process.exitCode = 1; });
}
