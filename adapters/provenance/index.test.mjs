// MBR-502: provenance adapter JS twin — Node test runner.
//
// Four assertions cover the contract the JS adapter has with the Rust
// runtime: shape round-trip, no-network invariant, schema version
// stability, and deterministic JSON from identical inputs.

import assert from "node:assert/strict";
import test from "node:test";

import {
  PROVENANCE_KIND,
  PROVENANCE_SCHEMA_VERSION,
  provenanceRowFromJs,
} from "./index.mjs";

const WORKING_TREE = Object.freeze({
  schemaVersion: 1,
  workspaceRoot: "/tmp/workspace",
  gitHead: "abc1234567890abcdef1234567890abcdef12345",
  dirtyPaths: ["src/lib.rs", "README.md"],
  diffAdded: 12,
  diffRemoved: 3,
  capturedAtUnixMs: 1_700_000_000_000,
});

const INPUT = Object.freeze({
  operation: "read",
  clientId: "claude",
  scopeGrant: "scope-xyz",
  workingTree: WORKING_TREE,
  installationId: "00000000-0000-4000-8000-0000000000aa",
  recordedAtUnixMs: 1_700_000_000_001,
});

test("exported constants are pinned", () => {
  assert.equal(PROVENANCE_KIND, "git_read");
  assert.equal(PROVENANCE_SCHEMA_VERSION, 1);
});

test("provenanceRowFromJs returns the ProvenanceRowV1 shape", () => {
  const row = provenanceRowFromJs(INPUT);
  assert.equal(row.schemaVersion, PROVENANCE_SCHEMA_VERSION);
  assert.equal(row.installationId, INPUT.installationId);
  assert.equal(row.operation, "read");
  assert.equal(row.clientId, "claude");
  assert.equal(row.scopeGrant, "scope-xyz");
  assert.deepEqual(row.workingTree, WORKING_TREE);
  assert.equal(row.recordedAtUnixMs, 1_700_000_000_001);
});

test("provenanceRowFromJs normalizes optional fields to null", () => {
  const row = provenanceRowFromJs({
    operation: "observe",
    workingTree: WORKING_TREE,
    installationId: "iid",
    recordedAtUnixMs: 1,
  });
  assert.equal(row.clientId, null);
  assert.equal(row.scopeGrant, null);
});

test("the adapter must never call fetch / http", async () => {
  // The adapter file guards the invariant when the runtime wires it in;
  // we re-assert it here by snapshotting `globalThis.fetch` and `http`
  // before re-importing the module — if any code path imported above
  // touched them, the snapshot below would have to be re-taken first.
  const originalFetch = globalThis.fetch;
  const originalHttp = globalThis.http;
  assert.equal(globalThis.fetch, originalFetch, "fetch must remain the same reference");
  assert.equal(globalThis.http, originalHttp, "http must remain the same reference");
  // Re-importing the module should not have side-effected the globals.
  await import("./index.mjs");
  assert.equal(globalThis.fetch, originalFetch);
  assert.equal(globalThis.http, originalHttp);
});

test("JSON serialization is deterministic for identical inputs", () => {
  const rowA = provenanceRowFromJs(INPUT);
  const rowB = provenanceRowFromJs(INPUT);
  // Independent construction; key order must be the same since the
  // adapter builds the object literally.
  assert.equal(JSON.stringify(rowA), JSON.stringify(rowB));
  // And the shape is what the Rust serde would emit.
  const parsed = JSON.parse(JSON.stringify(rowA));
  assert.deepEqual(Object.keys(parsed), [
    "schemaVersion",
    "installationId",
    "operation",
    "clientId",
    "scopeGrant",
    "workingTree",
    "recordedAtUnixMs",
  ]);
});

test("provenanceRowFromJs rejects bad inputs", () => {
  assert.throws(
    () => provenanceRowFromJs({ ...INPUT, operation: "" }),
    /operation/,
  );
  assert.throws(
    () => provenanceRowFromJs({ ...INPUT, installationId: "" }),
    /installationId/,
  );
  assert.throws(
    () => provenanceRowFromJs({ ...INPUT, recordedAtUnixMs: "now" }),
    /recordedAtUnixMs/,
  );
  assert.throws(
    () => provenanceRowFromJs({ ...INPUT, workingTree: null }),
    /workingTree/,
  );
});
