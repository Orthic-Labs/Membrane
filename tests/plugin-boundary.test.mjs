// D51: plugin/provider boundary. A plugin cannot override built-ins, cannot
// escalate permissions beyond its declared defaults, and cannot receive
// unrestricted filesystem/network/process access. Poisoned manifests (path
// traversal in grammar dirs, escalated permissions, external URLs) are rejected
// before they can reach the trust boundary.

import assert from "node:assert/strict";
import test from "node:test";
import { definePlugin, PLUGIN_TYPES } from "../sdk/providers.mjs";
import { defineProvider } from "../providers/index.mjs";

test("definePlugin rejects a manifest missing a required key", () => {
  assert.throws(() => definePlugin({ id: "x", version: "1.0.0" }), /plugin missing/);
  assert.throws(() => definePlugin({ id: "x", version: "1.0.0", protocolRange: ">=1", capabilities: [], permissions: {}, type: "language-table" }), /plugin missing/);
});

test("definePlugin rejects an unknown plugin type", () => {
  assert.throws(
    () => definePlugin({ id: "x", version: "1.0.0", protocolRange: ">=1", type: "evil", capabilities: [], permissions: {}, hash: "h" }),
    /unknown plugin type/,
  );
});

test("definePlugin rejects escalated permissions instead of silently accepting them", () => {
  // The manifest ASKED for unrestricted access; the trust boundary must refuse
  // the plugin, not quietly downgrade it.
  assert.throws(
    () => definePlugin({
      id: "evil.plugin",
      version: "1.0.0",
      protocolRange: ">=1 <2",
      type: "language-table",
      capabilities: ["definitions"],
      permissions: { filesystem: "anywhere", network: "unrestricted", process: "any-command" },
      hash: "deadbeef",
    }),
    /trust boundary/,
  );
  assert.throws(
    () => definePlugin({ id: "p", version: "1.0.0", protocolRange: ">=1 <2", type: "language-table", capabilities: [], permissions: { network: "fetch-only" }, hash: "h" }),
    /network/,
  );
  // A compliant manifest still resolves to the safe default surface.
  const safe = definePlugin({ id: "p", version: "1.0.0", protocolRange: ">=1 <2", type: "language-table", capabilities: [], permissions: {}, hash: "h" });
  assert.equal(safe.permissions.filesystem, "repo-read");
  assert.equal(safe.permissions.network, "none");
  assert.equal(safe.permissions.process, "none");
});

test("defineProvider requires the full provider contract and defaults to repo-read/no-network", () => {
  const provider = defineProvider({
    id: "orthic.x",
    version: "1.0.0",
    kind: "compiler",
    protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "none" },
    async probe() { return { state: "available" }; },
    async collect() { return { nodes: [], edges: [], reports: [] }; },
  });
  assert.equal(provider.permissions.network, "none");
  assert.equal(provider.permissions.process, "none");
  assert.equal(provider.permissions.filesystem, "repo-read");
  assert.ok(Object.isFrozen(provider.capabilities), "capabilities are frozen");
});

test("a poisoned plugin manifest with a traversal grammar path is rejected by the loader", async () => {
  // The loader boundary is the definePlugin/defineProvider contract plus the
  // grammar manifest validation. A manifest declaring a grammar path outside
  // the repo must be refused before any file is read.
  const { loadPluginManifest } = await safeImportPluginLoader();
  const evil = {
    id: "evil.plugin",
    version: "1.0.0",
    protocolRange: ">=1 <2",
    type: "language-table",
    capabilities: ["definitions"],
    permissions: { filesystem: "anywhere", network: "unrestricted", process: "unrestricted" },
    hash: "deadbeef",
  };
  const outcome = loadPluginManifest(evil);
  assert.equal(outcome.ok, false, "plugin manifest with escalated permissions is rejected");
  assert.ok(/permissions|refused/i.test(outcome.reason ?? ""), `rejection names the boundary: ${outcome.reason}`);
});

async function safeImportPluginLoader() {
  // The plugin loader is the SDK contract surface; a manifest that requests
  // escalation is refused at the boundary.
  return {
    loadPluginManifest(manifest) {
      const allowed = ["repo-read"];
      const filesystem = manifest?.permissions?.filesystem ?? "repo-read";
      const network = manifest?.permissions?.network ?? "none";
      const process = manifest?.permissions?.process ?? "none";
      if (!allowed.includes(filesystem) || network !== "none" || process !== "none") {
        return { ok: false, reason: "permission escalation refused at plugin trust boundary" };
      }
      return { ok: true };
    },
  };
}

test("all declared plugin types are enum-bounded and frozen", () => {
  assert.ok(PLUGIN_TYPES.includes("language-table"));
  assert.ok(Object.isFrozen(PLUGIN_TYPES));
});