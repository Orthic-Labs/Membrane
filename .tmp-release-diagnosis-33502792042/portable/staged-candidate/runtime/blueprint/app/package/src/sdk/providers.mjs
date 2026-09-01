// D32: plugin/provider contracts for the SDK. Plugins cannot override
// built-ins or receive unrestricted filesystem/network access.
//
// D51: a plugin manifest that DECLARES escalation is refused, never silently
// downgraded. The plugin trust boundary is the manifest itself: filesystem is
// repo-read only, network is none, process is none. `definePlugin` rejects any
// request that names anything broader — a poisoned manifest must fail the load
// loudly, because a silently-capable-but-requesting-more plugin is exactly the
// shape an attacker ships.

import { defineProvider as defineBaseProvider } from "../providers/index.mjs";

export { defineBaseProvider as defineProvider };

export const PLUGIN_TYPES = Object.freeze([
  "language-table",
  "compiler-provider",
  "framework-provider",
  "schema-iac-profile",
  "ranking-candidate",
  "rule-pack",
  "exporter",
  "provenance-provider",
]);

// The only permission surface a third-party plugin may hold. Anything broader
// is rejected — plugins never get host filesystem, network, or process access.
const ALLOWED_FILESYSTEM = new Set(["repo-read", "none"]);
const ALLOWED_NETWORK = new Set(["none"]);
const ALLOWED_PROCESS = new Set(["none"]);

function validatePermissions(plugin) {
  const requested = plugin.permissions ?? {};
  const filesystem = String(requested.filesystem ?? "repo-read");
  const network = String(requested.network ?? "none");
  const process = String(requested.process ?? "none");
  if (!ALLOWED_FILESYSTEM.has(filesystem)) return `permissions.filesystem "${filesystem}" exceeds plugin trust boundary (repo-read only)`;
  if (!ALLOWED_NETWORK.has(network)) return `permissions.network "${network}" exceeds plugin trust boundary (network: none)`;
  if (!ALLOWED_PROCESS.has(process)) return `permissions.process "${process}" exceeds plugin trust boundary (process: none)`;
  return null;
}

export function definePlugin(plugin) {
  for (const key of ["id", "version", "protocolRange", "capabilities", "permissions", "hash"]) {
    if (plugin[key] === undefined) throw new Error(`plugin missing ${key}`);
  }
  if (!PLUGIN_TYPES.includes(plugin.type)) throw new Error(`unknown plugin type ${plugin.type}`);
  const violation = validatePermissions(plugin);
  if (violation) throw new Error(violation);
  return Object.freeze({
    ...plugin,
    capabilities: Object.freeze([...plugin.capabilities]),
    permissions: Object.freeze({ filesystem: "repo-read", network: "none", process: "none", ...sanitizedPermissions(plugin.permissions) }),
  });
}

function sanitizedPermissions(permissions) {
  // Only the safe subset may survive the boundary; the validator above already
  // rejected anything broader, so this merely drops unknown extension keys.
  return {
    filesystem: permissions.filesystem ?? "repo-read",
    network: permissions.network ?? "none",
    process: permissions.process ?? "none",
  };
}
