import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  createReceiptStore,
  buildOrientationReceipt,
  receiptLookupKey,
  defaultReceiptStoreDir,
} from "../src/lib/receipt-store.mjs";

test("receipt store defaults outside the repo tree", () => {
  const dir = defaultReceiptStoreDir();
  assert.ok(!dir.includes(`${join("blueprint", ".agent")}`));
  assert.match(dir, /\.agent[/\\]receipts$/);
});

test("put/get/findActive keyed by session/task/repo/generation", () => {
  const storeDir = mkdtempSync(join(tmpdir(), "bp-receipts-"));
  try {
    const store = createReceiptStore({ storeDir });
    const receipt = buildOrientationReceipt({
      receiptId: store.nextId(),
      sessionId: "s1",
      taskId: "fix auth",
      repoIdentity: "demo@/tmp/demo",
      generationId: "xxh128:abc",
      manifestDigest: "sha256:def",
      candidateSetDigest: "sha256:ghi",
      allowedPaths: ["src/a.ts"],
    });
    store.put(receipt);
    assert.equal(store.get(receipt.receiptId).receiptId, receipt.receiptId);
    const found = store.findActive({
      sessionId: "s1",
      taskId: "fix auth",
      repoIdentity: "demo@/tmp/demo",
      generationId: "xxh128:abc",
    });
    assert.equal(found.receiptId, receipt.receiptId);
    assert.equal(
      receiptLookupKey(found),
      receiptLookupKey({
        sessionId: "s1",
        taskId: "fix auth",
        repoIdentity: "demo@/tmp/demo",
        generationId: "xxh128:abc",
      }),
    );
    assert.equal(
      store.findActive({
        sessionId: "s1",
        taskId: "other",
        repoIdentity: "demo@/tmp/demo",
        generationId: "xxh128:abc",
      }),
      null,
    );
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
  }
});

test("revoke marks receipt inactive without deleting the record", () => {
  const storeDir = mkdtempSync(join(tmpdir(), "bp-receipts-"));
  try {
    const store = createReceiptStore({ storeDir });
    const receipt = buildOrientationReceipt({
      receiptId: "r1",
      sessionId: "s",
      taskId: "t",
      repoIdentity: "repo",
      generationId: "g1",
    });
    store.put(receipt);
    const revoked = store.revoke("r1", { reason: "test" });
    assert.equal(revoked.status, "revoked");
    assert.equal(revoked.revokeReason, "test");
    assert.ok(existsSync(join(storeDir, "r1.json")));
    assert.equal(
      store.findActive({
        sessionId: "s",
        taskId: "t",
        repoIdentity: "repo",
        generationId: "g1",
      }),
      null,
    );
    assert.equal(store.list().length, 0);
    assert.equal(store.list({ includeRevoked: true }).length, 1);
    const raw = JSON.parse(readFileSync(join(storeDir, "r1.json"), "utf8"));
    assert.equal(raw.status, "revoked");
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
  }
});
