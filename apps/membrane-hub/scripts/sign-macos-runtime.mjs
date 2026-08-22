import { readdirSync, statSync } from "node:fs";
import { extname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const defaultRuntime = fileURLToPath(new URL("../src-tauri/runtime", import.meta.url));
const defaultIdentity = "Developer ID Application: Adrian D'souza (6KLGD3LLKF)";

export function signMacRuntime({
  runtimeDir = defaultRuntime,
  identity = process.env.APPLE_SIGNING_IDENTITY || defaultIdentity,
  run = command,
} = {}) {
  const candidates = walk(resolve(runtimeDir)).filter((file) => {
    const extension = extname(file);
    return [".node", ".dylib", ".so"].includes(extension) || (statSync(file).mode & 0o111) !== 0;
  });
  const machO = candidates.filter((file) => run("file", ["-b", file], { capture: true }).includes("Mach-O"));
  machO.sort((left, right) => right.split("/").length - left.split("/").length || left.localeCompare(right));

  for (const file of machO) {
    run("codesign", ["--force", "--options", "runtime", "--timestamp", "--sign", identity, file]);
    const details = run("codesign", ["-dv", "--verbose=4", file], { capture: true });
    if (!/TeamIdentifier=6KLGD3LLKF/.test(details)) throw new Error(`Developer ID team mismatch: ${file}`);
    if (!/^Timestamp=/m.test(details)) throw new Error(`secure timestamp missing: ${file}`);
    run("codesign", ["--verify", "--strict", "--verbose=2", file]);
  }

  console.log(`[membrane] signed ${machO.length} staged runtime Mach-O file(s)`);
  return machO;
}

function walk(root) {
  const output = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const absolute = join(root, entry.name);
    if (entry.isDirectory()) output.push(...walk(absolute));
    else if (entry.isFile()) output.push(absolute);
  }
  return output;
}

function command(executable, args, { capture = false } = {}) {
  const result = spawnSync(executable, args, { encoding: capture ? "utf8" : undefined, stdio: capture ? "pipe" : "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} failed with exit ${result.status}`);
  return capture ? `${result.stdout ?? ""}${result.stderr ?? ""}` : "";
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) signMacRuntime();
