import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

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
  return resolveTargetRoot(manifestPath);
}
