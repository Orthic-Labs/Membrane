// D30: daemon concurrency — concurrent clients receive bounded,
// generation-consistent responses; one repo failure does not starve others.

import assert from "node:assert/strict";
import { mkdtempSync, cpSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { createDaemonServer } from "../service/server.mjs";
import { DaemonClient } from "../service/client.mjs";
import { temporaryDaemonEndpoint } from "../service/paths.mjs";
import { buildGraphGeneration } from "../graph/static-provider.mjs";
import { createCortexApplicationService } from "../lib/application/service.mjs";
import { RootRegistry } from "../lib/application/root-registry.mjs";

function controlledFreshnessService({ onFreshness = () => {}, onStatus = () => {} } = {}) {
  return {
    async openFreshnessSession(input) {
      return await onFreshness(input) ?? { root: input.repoRoot, close() {} };
    },
    async status(input, options) {
      return onStatus(input, options);
    },
  };
}

test("concurrent clients get consistent responses", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-ipc-conc-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const endpoint = temporaryDaemonEndpoint("cortex-conc");
  const registry = new RootRegistry([{ root: repo, repoId: "repo-1" }]);
  const service = createCortexApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const daemon = createDaemonServer({ service, endpoint });
  try {
    await daemon.listen();
    const clients = [new DaemonClient({ endpoint }), new DaemonClient({ endpoint }), new DaemonClient({ endpoint })];
    const results = await Promise.all(clients.map((client) => client.request({ method: "search", input: { query: "placeOrder", limit: 5 }, repoId: "repo-1" })));
    for (const response of results) {
      assert.equal(response.ok, true);
      assert.ok(response.result.generationId, "generation-consistent response");
    }
    await Promise.all(clients.map((client) => client.close()));
  } finally {
    await daemon.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("a failing repo returns typed errors without starving others", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-ipc-fail-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const endpoint = temporaryDaemonEndpoint("cortex-fail");
  const registry = new RootRegistry([
    { root: repo, repoId: "repo-1" },
    { root: join(tmpdir(), "missing-repo"), repoId: "repo-missing" },
  ]);
  const service = createCortexApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const daemon = createDaemonServer({ service, endpoint });
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const [bad, good] = await Promise.all([
      client.request({ method: "search", input: { query: "x", limit: 5 }, repoId: "repo-missing" }),
      client.request({ method: "search", input: { query: "placeOrder", limit: 5 }, repoId: "repo-1" }),
    ]);
    assert.equal(bad.ok, false);
    assert.ok(bad.error.code, "typed error for the failing repo");
    assert.equal(good.ok, true);
    assert.ok(good.result.results.length > 0, "healthy repo still serves");
    await client.close();
  } finally {
    await daemon.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("injected real application service serializes same-root freshness without duplicate registry entries", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-ipc-injected-real-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const endpoint = temporaryDaemonEndpoint("cortex-injected-real");
  const realService = createCortexApplicationService({ rootRegistry: new RootRegistry([{ root: repo, repoId: "repo" }]), allowEmbeddedRoot: false });
  const started = [];
  const releases = [];
  const service = {
    ...realService,
    async openFreshnessSession(input) {
      started.push(input.repoRoot);
      return new Promise((resolve) => releases.push(resolve));
    },
  };
  const daemon = createDaemonServer({ service, endpoint });
  const requests = [];
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const first = client.request({ method: "status", repoId: "repo" });
    const second = client.request({ method: "status", repoId: "repo" });
    requests.push(first, second);
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.deepEqual(started, [repo]);
    releases.shift()({ close() {} });
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.deepEqual(started, [repo, repo]);
    for (const release of releases.splice(0)) release({ close() {} });
    await Promise.all([first, second]);
    await client.close();
  } finally {
    for (const release of releases.splice(0)) release({ close() {} });
    await Promise.allSettled(requests);
    await daemon.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("daemon serializes freshness sessions for one canonical root while another root proceeds", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-root-fifo");
  const firstRoot = join(tmpdir(), "cortex-root-fifo-a");
  const secondRoot = join(tmpdir(), "cortex-root-fifo-b");
  const started = [];
  const releases = [];
  const service = controlledFreshnessService({
    onFreshness(input) {
      started.push(input.repoRoot);
      return new Promise((resolve) => releases.push(resolve));
    },
    onStatus: async () => ({ generationId: "g" }),
  });
  const daemon = createDaemonServer({
    endpoint,
    service,
    registryEntries: [
      { root: firstRoot, repoId: "first" },
      { root: secondRoot, repoId: "second" },
    ],
  });
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const first = client.request({ method: "status", repoId: "first" });
    const sameRoot = client.request({ method: "status", repoId: "first" });
    const otherRoot = client.request({ method: "status", repoId: "second" });
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.deepEqual(started.sort(), [firstRoot, secondRoot].sort());
    releases.shift()({ root: firstRoot, close() {} });
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.equal(started.filter((root) => root === firstRoot).length, 2);
    for (const release of releases.splice(0)) release({ root: secondRoot, close() {} });
    await Promise.all([first, sameRoot, otherRoot]);
    await client.close();
  } finally {
    for (const release of releases.splice(0)) release({ close() {} });
    await daemon.close().catch(() => {});
  }
});

test("daemon allows same-root traversal work after its freshness sessions open", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-traversal-concurrency");
  const root = join(tmpdir(), "cortex-traversal-concurrency");
  const started = [];
  const releases = [];
  const service = controlledFreshnessService({
    onStatus(_input, { session }) {
      started.push(session.root);
      return new Promise((resolve) => releases.push(resolve));
    },
  });
  const daemon = createDaemonServer({ endpoint, service, registryEntries: [{ root, repoId: "repo" }] });
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const first = client.request({ method: "status", repoId: "repo", deadlineMs: 30000 });
    const second = client.request({ method: "status", repoId: "repo", deadlineMs: 30000 });
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.deepEqual(started, [root, root]);
    for (const release of releases.splice(0)) release({ generationId: "g" });
    await Promise.all([first, second]);
    await client.close();
  } finally {
    for (const release of releases.splice(0)) release({ generationId: "g" });
    await daemon.close().catch(() => {});
  }
});

test("daemon queues repository aliases on their shared canonical root", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-alias-fifo");
  const root = join(tmpdir(), "cortex-alias-fifo");
  const started = [];
  const releases = [];
  const service = controlledFreshnessService({
    onFreshness(input) {
      started.push(input.repoRoot);
      return new Promise((resolve) => releases.push(resolve));
    },
    onStatus: async () => ({ generationId: "g" }),
  });
  const daemon = createDaemonServer({
    endpoint,
    service,
    registryEntries: [{ root, repoId: "primary" }, { root, repoId: "alias" }],
  });
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const primary = client.request({ method: "status", repoId: "primary" });
    const alias = client.request({ method: "status", repoId: "alias" });
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.deepEqual(started, [root]);
    releases.shift()({ root, close() {} });
    await new Promise((resolve) => setTimeout(resolve, 25));
    assert.deepEqual(started, [root, root]);
    for (const release of releases.splice(0)) release({ root, close() {} });
    await Promise.all([primary, alias]);
    await client.close();
  } finally {
    for (const release of releases.splice(0)) release({ close() {} });
    await daemon.close().catch(() => {});
  }
});
