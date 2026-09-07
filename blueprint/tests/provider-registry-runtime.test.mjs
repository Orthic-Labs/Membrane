import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  ProviderRegistry,
  defineProvider,
  providerDescriptorDigest,
  runProvider,
  runProviderSync,
  validateProviderManifest,
} from "../src/providers/index.mjs";

function provider(overrides = {}) {
  return defineProvider({
    id: "test.provider",
    version: "1.2.3",
    kind: "structural",
    protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "none" },
    async probe() { return { state: "available" }; },
    async collect() { return { nodes: [], edges: [], reports: [] }; },
    ...overrides,
  });
}

test("provider contract normalizes identity, permissions and descriptor digest", () => {
  const value = provider();
  assert.match(value.descriptorDigest, /^sha256:[0-9a-f]{64}$/);
  assert.equal(value.descriptorDigest, providerDescriptorDigest(value));
  assert.ok(Object.isFrozen(value));
  assert.ok(Object.isFrozen(value.capabilities));
  assert.ok(Object.isFrozen(value.permissions));
  assert.throws(() => provider({ permissions: { filesystem: "anywhere", network: "none", process: "none" } }), { code: "provider_permissions_invalid" });
  assert.throws(() => provider({ permissions: { filesystem: "repo-read", network: "fetch", process: "none" } }), { code: "provider_permissions_invalid" });
});

test("provider manifest checksum, licence and entry are fail-closed", () => {
  const bytes = Buffer.from("export default 1;\n");
  const integrity = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  const manifest = { id: "test.provider", version: "1.2.3", license: "MIT", integrity, entry: "providers/test.mjs" };
  assert.equal(validateProviderManifest(manifest, { artifactBytes: bytes, allowedLicenses: ["MIT"] }).entry, "providers/test.mjs");
  assert.throws(() => validateProviderManifest({ ...manifest, entry: "../escape.mjs" }, { artifactBytes: bytes }), { code: "provider_manifest_entry_invalid" });
  assert.throws(() => validateProviderManifest({ ...manifest, integrity: `sha256:${"0".repeat(64)}` }, { artifactBytes: bytes }), { code: "provider_integrity_mismatch" });
  assert.throws(() => validateProviderManifest(manifest, { artifactBytes: bytes, allowedLicenses: ["Apache-2.0"] }), { code: "provider_license_rejected" });
});

test("registry rejects duplicate and manifest/provider identity mismatch", () => {
  const value = provider();
  const registry = new ProviderRegistry();
  registry.register(value);
  assert.equal(registry.get(value.id).provider, value);
  assert.deepEqual(registry.capability("definitions").map((entry) => entry.provider.id), ["test.provider"]);
  assert.throws(() => registry.register(value), { code: "provider_duplicate" });

  const bytes = Buffer.from("x");
  const integrity = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  const second = new ProviderRegistry({ allowedLicenses: ["MIT"] });
  assert.throws(() => second.register(value, {
    manifest: { id: "other.provider", version: "1.2.3", license: "MIT", integrity, entry: "provider.mjs" },
    artifactBytes: bytes,
  }), { code: "provider_manifest_identity_mismatch" });
});

test("runtime types process authorization, crash, timeout and cancellation", async () => {
  await assert.rejects(
    runProvider(provider({ permissions: { filesystem: "repo-read", network: "none", process: "opt-in" } }), {}, { timeoutMs: 50 }),
    { code: "provider_process_not_authorized" },
  );
  await assert.rejects(
    runProvider(provider({ async collect() { throw new Error("boom"); } }), {}, { timeoutMs: 50 }),
    { code: "provider_crash" },
  );
  await assert.rejects(
    runProvider(provider({ async collect() { return new Promise(() => {}); } }), {}, { timeoutMs: 10 }),
    { code: "provider_timeout" },
  );
  const controller = new AbortController();
  const pending = runProvider(provider({ async collect({ signal }) {
    return new Promise((resolve) => signal.addEventListener("abort", () => resolve({ state: "cancelled" }), { once: true }));
  } }), {}, { signal: controller.signal, timeoutMs: 100 });
  controller.abort();
  await assert.rejects(pending, { code: "provider_cancelled" });
});

// BPT-012: the synchronous lane the production build uses must apply the same
// admission bounds. A frozen object carrying a `descriptorDigest` bypasses
// `defineProvider`, so the permission guards must re-check rather than trust it.
function forgedDefinedProvider(permissions, overrides = {}) {
  return Object.freeze({
    id: "forged.provider", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: Object.freeze(["definitions"]),
    permissions: Object.freeze(permissions),
    descriptorDigest: `sha256:${"a".repeat(64)}`,
    probe() { return { state: "available" }; },
    collect() { return { nodes: [], edges: [], reports: [] }; },
    ...overrides,
  });
}

test("synchronous provider lane refuses network, invalid filesystem and unauthorized process", () => {
  assert.throws(() => runProviderSync(forgedDefinedProvider({ filesystem: "repo-read", network: "fetch-only", process: "none" })), { code: "provider_network_forbidden" });
  assert.throws(() => runProviderSync(forgedDefinedProvider({ filesystem: "anywhere", network: "none", process: "none" })), { code: "provider_filesystem_forbidden" });
  assert.throws(() => runProviderSync(provider({ permissions: { filesystem: "repo-read", network: "none", process: "opt-in" }, collect() { return {}; } })), { code: "provider_process_not_authorized" });
  assert.deepEqual(runProviderSync(provider({ permissions: { filesystem: "repo-read", network: "none", process: "opt-in" }, collect() { return { ok: true }; } }), {}, { allowProcess: true }), { ok: true });
});

test("synchronous provider lane types crashes, refuses async bodies and honours a pre-aborted signal", () => {
  assert.throws(() => runProviderSync(provider({ collect() { throw new Error("boom"); } })), { code: "provider_crash" });
  assert.throws(() => runProviderSync(provider({ async collect() { return {}; } })), { code: "provider_sync_required" });
  const controller = new AbortController();
  controller.abort();
  assert.throws(() => runProviderSync(provider({ collect() { return {}; } }), {}, { signal: controller.signal }), { code: "provider_cancelled" });
});
