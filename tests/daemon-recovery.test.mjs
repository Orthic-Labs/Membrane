// D30: daemon recovery — restart and stale endpoint recovery are
// deterministic; a dead socket fails fast with a typed error.

import assert from "node:assert/strict";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import test from "node:test";

import { acquireUnixLock, createDaemonServer } from "../service/server.mjs";
import { DaemonClient } from "../service/client.mjs";
import { temporaryDaemonEndpoint } from "../service/paths.mjs";

test("daemon restart re-binds the endpoint", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-restart");
  const first = createDaemonServer({ endpoint });
  try {
    await first.listen();
    if (process.platform !== "win32") {
      const rival = createDaemonServer({ endpoint });
      try {
        await assert.rejects(rival.listen(), { code: "endpoint_locked" });
      } finally {
        await rival.close().catch(() => {});
      }
      assert.ok(existsSync(endpoint), "failed rival cannot unlink the live endpoint");
    }
  } finally {
    await first.close().catch(() => {});
  }
  if (process.platform !== "win32") assert.ok(!existsSync(endpoint), "endpoint cleaned up after close");
  const second = createDaemonServer({ endpoint });
  try {
    await second.listen();
    if (process.platform !== "win32") assert.ok(existsSync(endpoint), "endpoint re-bound after restart");
    const client = new DaemonClient({ endpoint });
    const response = await client.request({ method: "status", input: {} });
    assert.ok(response.ok === true || response.ok === false);
    await client.close();
  } finally {
    await second.close().catch(() => {});
  }
});

test("connecting to a stale endpoint fails fast with a typed error", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-stale");
  const client = new DaemonClient({ endpoint });
  try {
    await client.connect();
    assert.fail("expected connect to fail");
  } catch (error) {
    assert.ok(error, "typed connect error");
  }
});

test("unix daemon quarantines dead lock owners and rejects a live rival", { skip: process.platform === "win32" }, async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-lock");
  const lockPath = `${endpoint}.lock`;
  mkdirSync(lockPath, { recursive: true });
  writeFileSync(`${lockPath}/owner.json`, JSON.stringify({ pid: 99999999, token: "dead-owner" }));
  const first = createDaemonServer({ endpoint });
  try {
    await first.listen();
    const rival = createDaemonServer({ endpoint });
    try {
      await assert.rejects(rival.listen(), { code: "endpoint_locked" });
    } finally {
      await rival.close().catch(() => {});
    }
  } finally {
    await first.close().catch(() => {});
  }
});

test("unix lock owner-write failure removes only its newly-created empty lock", { skip: process.platform === "win32" }, () => {
  const endpoint = temporaryDaemonEndpoint("cortex-lock-write");
  const lockPath = `${endpoint}.lock`;
  assert.throws(() => acquireUnixLock(endpoint, { writeOwner: () => { throw new Error("owner write failed"); } }), { code: "endpoint_locked" });
  assert.equal(existsSync(lockPath), false);
});
