// D51: plugin/provider boundary. A plugin cannot override built-ins, cannot
// escalate permissions beyond its declared defaults, and cannot receive
// unrestricted filesystem/network/process access. Poisoned manifests (path
// traversal in grammar dirs, escalated permissions, external URLs) are rejected
// before they can reach the trust boundary.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { definePlugin, PLUGIN_TYPES } from "../src/sdk/providers.mjs";
import { defineProvider } from "../src/providers/index.mjs";
import { admitPluginManifest, PLUGIN_MANIFEST_DIR } from "../src/providers/plugin-loader.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const LICENSE = "SEE LICENSE IN LICENSE";

/** Fixture repository shipping plugin manifests and the artifacts they name. */
function pluginRepo(manifests, files = {}) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-plugin-"));
  mkdirSync(join(root, PLUGIN_MANIFEST_DIR), { recursive: true });
  for (const [name, manifest] of Object.entries(manifests)) {
    writeFileSync(join(root, PLUGIN_MANIFEST_DIR, name), JSON.stringify(manifest, null, 2));
  }
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, path, ".."), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}

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
    id: "membrane.x",
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

test("the real loader refuses a plugin manifest that declares escalated permissions", () => {
  // This assertion used to run against a `loadPluginManifest` closure written
  // inside this test file, so it proved only that the test's own copy of the
  // rule worked. It now drives the production admission gate.
  const root = pluginRepo({
    "evil.json": {
      id: "evil.plugin",
      version: "1.0.0",
      type: "language-table",
      license: LICENSE,
      integrity: `sha256:${"0".repeat(64)}`,
      entry: "plugins/evil.mjs",
      capabilities: ["definitions"],
      permissions: { filesystem: "anywhere", network: "unrestricted", process: "any-command" },
    },
  }, { "plugins/evil.mjs": "export default {};\n" });
  try {
    const outcome = admitPluginManifest(root, join(PLUGIN_MANIFEST_DIR, "evil.json"));
    assert.equal(outcome.disposition, "refused");
    assert.equal(outcome.code, "plugin_permission_refused");
    assert.match(outcome.reason, /trust boundary/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the real loader refuses a plugin entry that escapes the repository", () => {
  const root = pluginRepo({
    "traversal.json": {
      id: "traversal.plugin",
      version: "1.0.0",
      type: "language-table",
      license: LICENSE,
      integrity: `sha256:${"0".repeat(64)}`,
      entry: "../../../etc/passwd",
      capabilities: [],
      permissions: {},
    },
  });
  try {
    const outcome = admitPluginManifest(root, join(PLUGIN_MANIFEST_DIR, "traversal.json"));
    assert.equal(outcome.disposition, "refused");
    // The entry is rejected as non-repository-relative before it is opened.
    assert.equal(outcome.code, "provider_manifest_entry_invalid");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the real loader refuses a plugin whose artifact does not match its declared checksum", () => {
  const root = pluginRepo({
    "swapped.json": {
      id: "swapped.plugin",
      version: "1.0.0",
      type: "language-table",
      license: LICENSE,
      entry: "plugins/swapped.mjs",
      integrity: `sha256:${"0".repeat(64)}`,
      capabilities: [],
      permissions: {},
    },
  }, { "plugins/swapped.mjs": "export default { swapped: true };\n" });
  try {
    const outcome = admitPluginManifest(root, join(PLUGIN_MANIFEST_DIR, "swapped.json"));
    assert.equal(outcome.disposition, "refused");
    assert.equal(outcome.code, "plugin_integrity_mismatch");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the real loader admits a compliant plugin and clamps it to the trust boundary", () => {
  const entry = "export default { ok: true };\n";
  const integrity = `sha256:${createHash("sha256").update(entry).digest("hex")}`;
  const root = pluginRepo({
    "good.json": {
      id: "good.plugin",
      version: "1.0.0",
      type: "language-table",
      license: LICENSE,
      entry: "plugins/good.mjs",
      integrity,
      capabilities: ["definitions"],
      permissions: { filesystem: "repo-read" },
    },
  }, { "plugins/good.mjs": entry });
  try {
    const outcome = admitPluginManifest(root, join(PLUGIN_MANIFEST_DIR, "good.json"), {
      allowedLicenses: [LICENSE],
    });
    assert.equal(outcome.disposition, "admitted", JSON.stringify(outcome));
    assert.equal(outcome.id, "good.plugin");
    assert.deepEqual({ ...outcome.permissions }, { filesystem: "repo-read", network: "none", process: "none" });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("plugin admission is part of the sealed generation, so a refusal is never silent", () => {
  const root = pluginRepo({
    "evil.json": {
      id: "evil.plugin",
      version: "1.0.0",
      type: "language-table",
      license: LICENSE,
      integrity: `sha256:${"0".repeat(64)}`,
      entry: "plugins/evil.mjs",
      capabilities: [],
      permissions: { network: "unrestricted" },
    },
  }, { "plugins/evil.mjs": "export default {};\n", "src/a.js": "export const a = 1;\n" });
  try {
    const generation = buildGraphGeneration(root, { outDir: ".agent", persist: false });
    const plugins = generation.augmentation.providers.plugins;
    assert.equal(plugins.considered, 1);
    assert.equal(plugins.admitted.length, 0);
    assert.equal(plugins.refused.length, 1, "the refusal travels in the sealed manifest");
    assert.equal(plugins.refused[0].code, "plugin_permission_refused");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("all declared plugin types are enum-bounded and frozen", () => {
  assert.ok(PLUGIN_TYPES.includes("language-table"));
  assert.ok(Object.isFrozen(PLUGIN_TYPES));
});
