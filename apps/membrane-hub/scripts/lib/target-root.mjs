import { dirname, isAbsolute, resolve } from "node:path";

// Native release commands pass the same explicit target root to direct Cargo
// and staging. Never invoke Cargo metadata here: it can route through a host
// shim and silently mix another build authority into release packaging.
export function resolveManagedCargoTarget(manifestPath) {
  const workspace = dirname(resolve(manifestPath));
  const configured = process.env.MEMBRANE_CARGO_TARGET_DIR;
  return configured ? (isAbsolute(configured) ? resolve(configured) : resolve(workspace, configured)) : resolve(workspace, "target");
}
