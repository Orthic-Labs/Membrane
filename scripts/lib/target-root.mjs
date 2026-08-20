import { execFileSync } from "node:child_process";
import path from "node:path";

// A managed build owns the target root, so `src-tauri/target` is not where the
// output lands. `cargo metadata`'s target_directory is the sole source of
// truth for locating it — a broker-managed CARGO_TARGET_DIR can be set but
// stale, so it must never be used to locate output. Fail closed if metadata
// cannot be resolved, naming the manifest path.
export function cargoTargetRoot(appRoot) {
  const manifestPath = path.join(appRoot, "src-tauri", "Cargo.toml");
  let raw;
  try {
    raw = execFileSync("cargo", [
      "metadata", "--manifest-path", "Cargo.toml", "--no-deps", "--offline", "--format-version", "1",
    ], { cwd: path.join(appRoot, "src-tauri"), encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  } catch (err) {
    throw new Error(`cargo metadata failed for ${manifestPath}: ${err.message}`);
  }
  const directory = JSON.parse(raw).target_directory;
  if (typeof directory !== "string" || !directory || !path.isAbsolute(directory)) {
    throw new Error(`cargo metadata for ${manifestPath} did not report an absolute target_directory`);
  }
  return path.resolve(directory);
}
