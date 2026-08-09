// D28/D32: provider interface (S-20). Every provider declares id/version/kind/
// protocol range/capabilities/permissions and defaults to repo-read
// filesystem, no network, no process.

export function defineProvider(provider) {
  for (const key of ["id", "version", "kind", "protocolRange", "capabilities", "permissions", "probe", "collect"]) {
    if (provider[key] === undefined) throw new Error(`provider missing ${key}`);
  }
  return Object.freeze({
    ...provider,
    capabilities: Object.freeze([...provider.capabilities]),
    permissions: Object.freeze({ filesystem: "repo-read", network: "none", process: "none", ...provider.permissions }),
  });
}

export const example = defineProvider({
  id: "orthic.typescript",
  version: "1.0.0",
  kind: "compiler",
  protocolRange: ">=1 <2",
  capabilities: ["definitions", "references", "types"],
  permissions: { filesystem: "repo-read", network: "none", process: "opt-in" },
  async probe(context) { return { state: "available", details: {} }; },
  async collect(context) { return { nodes: [], edges: [], reports: [] }; },
});
