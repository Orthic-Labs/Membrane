import { spawnSync } from "node:child_process";
import path from "node:path";

// `cargo metadata`'s `target_directory` is the SOLE source of truth for
// locating a managed Cargo crate's build output. On a broker-managed host
// process.env.CARGO_TARGET_DIR can be SET BUT STALE (left over from an
// earlier session), so trusting it directly for *location* silently reads,
// packages, or publishes the wrong build tree. Cargo's own metadata already
// resolves the same env var, `.cargo/config.toml`, and workspace settings
// the way a real `cargo build` would, so asking cargo is both safer and
// authoritative. If metadata resolution fails we fail closed with the exact
// manifest path attempted — never fall back to a guessed or hardcoded path.
// Mirrors `resolveManagedCargoTarget` in
// tools/rightkit/packages/release/build-release.mjs.
export function resolveManagedCargoTarget(manifestPath) {
  if (!manifestPath) throw new Error("resolveManagedCargoTarget requires a Cargo.toml manifest path");
  const result = spawnSync(
    "cargo",
    ["metadata", "--no-deps", "--offline", "--format-version", "1", "--manifest-path", manifestPath],
    { cwd: path.dirname(manifestPath), encoding: "utf8", windowsHide: true },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `cargo metadata failed for ${manifestPath}: ${(result.stderr ?? "").trim() || `exit ${result.status}`}`,
    );
  }
  const target = JSON.parse(result.stdout).target_directory;
  if (!target || !path.isAbsolute(target)) {
    throw new Error(`cargo metadata for ${manifestPath} returned no absolute target directory`);
  }
  return target;
}
