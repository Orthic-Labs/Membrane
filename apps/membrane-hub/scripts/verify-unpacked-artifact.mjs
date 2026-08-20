// Native-host artifact proof. NSIS is installed silently into a temporary
// directory, then only packaged runtime bytes/sidecars are passed to probes.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { verifyUnpackedArtifact } from "./runtime-inventory.mjs";

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`unpacked artifact setup failed: ${command}`);
}
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

export async function verifyPackagedArtifact({ app, nsis } = {}) {
  if (Boolean(app) === Boolean(nsis)) throw new Error("supply exactly one of --app or --nsis");
  if (app) return verifyUnpackedArtifact(appLayout(resolve(app)));
  if (process.platform !== "win32") throw new Error("NSIS unpacked proof requires Windows");
  const root = mkdtempSync(join(tmpdir(), "membrane-hub-nsis-probe-"));
  try { run(resolve(nsis), ["/S", `/D=${join(root, "Membrane")}`]); return await verifyUnpackedArtifact(findRuntime(root)); }
  finally { rmSync(root, { recursive: true, force: true }); }
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  verifyPackagedArtifact(args(process.argv.slice(2))).catch((error) => { console.error(error.message); process.exitCode = 1; });
}
