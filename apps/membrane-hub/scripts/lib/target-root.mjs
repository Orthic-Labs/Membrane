import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

// Keep existing consumers on one helper name while RightKit remains sole
// authority for broker-managed and local Cargo target resolution.
export function resolveManagedCargoTarget(manifestPath) {
  return resolveTargetRoot(manifestPath);
}
