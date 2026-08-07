// tests/install/uninstall-js.test.mjs — MBR-205.
//
// Verifies the JS mirror of the ownership-safe uninstall contract. The
// test loads the helpers from `mcp/install.mjs` and asserts:
//
//   * an empty ownership table refuses every candidate,
//   * a registered candidate is authorised, an unregistered one is refused,
//   * duplicate `(kind, path)` registration surfaces a typed error,
//   * the `installation_id` is stable per receipt root and distinct across roots.
//
// Pure in-process: no native MCP CLI or membrane binary is invoked, so the
// test is fast and deterministic.

import assert from "node:assert/strict";
import test from "node:test";
import {
  authorizeUninstall,
  DuplicateOwnershipError,
  freshOwnershipTable,
  loadOwnershipTable,
  OWNERSHIP_KINDS,
  registerOwnershipClaim,
} from "../../mcp/install.mjs";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function freshRoot(label) {
  return mkdtempSync(join(tmpdir(), `mbr-205-js-${label}-`));
}

test("empty table refuses every candidate", () => {
  const table = freshOwnershipTable("/var/membrane/example");
  const authorised = authorizeUninstall(table, [
    "/var/membrane/manifest.json",
    "/var/membrane/lease.json",
  ]);
  assert.deepEqual(authorised, []);
});

test("registered candidate is authorised; unregistered candidate is refused", () => {
  const table = freshOwnershipTable("/var/membrane/example");
  registerOwnershipClaim(table, {
    kind: OWNERSHIP_KINDS.Manifest,
    path: "/var/membrane/manifest.json",
    receiptId: "plan-1",
    registeredAtUnixMs: 1_755_000_000_000,
  });
  registerOwnershipClaim(table, {
    kind: OWNERSHIP_KINDS.Lease,
    path: "/var/membrane/lease.json",
    receiptId: "sup-1",
    registeredAtUnixMs: 1_755_000_000_000,
  });

  const authorised = authorizeUninstall(table, [
    "/var/membrane/manifest.json",
    "/var/membrane/stray.txt",
    "/var/membrane/lease.json",
  ]);
  assert.deepEqual(authorised, [
    "/var/membrane/manifest.json",
    "/var/membrane/lease.json",
  ]);
});

test("duplicate (kind, path) registration surfaces DuplicateOwnershipError", () => {
  const table = freshOwnershipTable("/var/membrane/example");
  const claim = {
    kind: OWNERSHIP_KINDS.Manifest,
    path: "/var/membrane/manifest.json",
    receiptId: "plan-1",
    registeredAtUnixMs: 1_755_000_000_000,
  };
  registerOwnershipClaim(table, claim);
  assert.throws(
    () => registerOwnershipClaim(table, claim),
    (error) => {
      assert.ok(error instanceof DuplicateOwnershipError, `expected DuplicateOwnershipError, got ${error?.name}`);
      assert.equal(error.kind, OWNERSHIP_KINDS.Manifest);
      assert.equal(error.path, "/var/membrane/manifest.json");
      return true;
    },
  );
  // Same path with a different kind is permitted.
  registerOwnershipClaim(table, {
    kind: OWNERSHIP_KINDS.Binding,
    path: "/var/membrane/manifest.json",
    receiptId: "plan-1",
    registeredAtUnixMs: 1_755_000_000_000,
  });
  // Same kind with a different path is permitted.
  registerOwnershipClaim(table, {
    kind: OWNERSHIP_KINDS.Manifest,
    path: "/var/membrane/another.json",
    receiptId: "plan-1",
    registeredAtUnixMs: 1_755_000_000_000,
  });
  assert.equal(table.claims.length, 3);
});

test("installation_id is stable per receipt root and distinct across roots", () => {
  const a = freshOwnershipTable("/var/membrane/a");
  const b = freshOwnershipTable("/var/membrane/b");
  const aAgain = freshOwnershipTable("/var/membrane/a");
  assert.ok(a.installationId.startsWith("sha256:"));
  assert.equal(a.installationId, aAgain.installationId);
  assert.notEqual(a.installationId, b.installationId);
});

test("loadOwnershipTable returns fresh table when the file is missing", async () => {
  const root = freshRoot("missing");
  try {
    const table = await loadOwnershipTable(root);
    assert.ok(table.installationId.startsWith("sha256:"));
    assert.deepEqual(table.claims, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("loadOwnershipTable reads the on-disk table", async () => {
  const root = freshRoot("on-disk");
  try {
    const table = freshOwnershipTable(root);
    registerOwnershipClaim(table, {
      kind: OWNERSHIP_KINDS.Receipt,
      path: "/var/membrane/install-receipt.json",
      receiptId: "plan-1",
      registeredAtUnixMs: 1_755_000_000_000,
    });
    writeFileSync(
      join(root, "ownership.json"),
      JSON.stringify(table, null, 2),
      "utf8",
    );
    const reloaded = await loadOwnershipTable(root);
    assert.equal(reloaded.installationId, table.installationId);
    assert.equal(reloaded.claims.length, 1);
    assert.equal(reloaded.claims[0].path, "/var/membrane/install-receipt.json");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
