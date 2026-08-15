import { execFileSync } from "node:child_process";
import path from "node:path";

// A managed build owns the target root, so `src-tauri/target` is not where the
// output lands. Honour the injected directory and otherwise ask Cargo — the
// same resolution HeardRight's scripts/lib/target-root.mjs uses.
export function cargoTargetRoot(appRoot) {
  if (process.env.CARGO_TARGET_DIR) return path.resolve(appRoot, process.env.CARGO_TARGET_DIR);
  const raw = execFileSync("cargo", [
    "metadata", "--manifest-path", "Cargo.toml", "--no-deps", "--format-version", "1",
  ], { cwd: path.join(appRoot, "src-tauri"), encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  const directory = JSON.parse(raw).target_directory;
  if (typeof directory !== "string" || !directory) throw new Error("cargo metadata did not report target_directory");
  return path.resolve(directory);
}
