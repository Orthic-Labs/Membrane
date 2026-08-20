import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { SNAPSHOT_SCHEMA_VERSION, buildSnapshot, startSnapshotServer, validateSnapshot } from "../src/lib/orthic-snapshot.mjs";

test("snapshot builds with required fields", () => {
  const snap = buildSnapshot({ root: process.cwd(), outDir: ".agent" });
  assert.equal(snap.schemaVersion, SNAPSHOT_SCHEMA_VERSION);
  assert.equal(snap.productId, "blueprint");
  assert.ok(snap.observedAtUnixMs);
  assert.ok(snap.sections);
  assert.ok(snap.sections.watcher);
  assert.ok(snap.sections.graph);
  for (const section of Object.values(snap.sections)) assert.ok(section.reason);
});

test("payload validates against schema", async () => {
  const snap = buildSnapshot({ root: process.cwd() });
  const schema = JSON.parse(readFileSync("schemas/orthic-product-snapshot-v2.schema.json", "utf8"));
  const { default: Ajv } = await import("ajv");
  const ajv = new Ajv();
  const validate = ajv.compile(schema);
  assert.equal(validate(snap), true, JSON.stringify(validate.errors));
});

test("missing-input section reports unavailable with reason, not fabricated zero", () => {
  const snap = buildSnapshot({ root: "/nonexistent/path/that/does/not/exist" });
  assert.equal(snap.sections.graph.state, "unavailable");
  assert.ok(snap.sections.graph.reason);
  assert.equal(typeof snap.sections.graph.reason, "string");
});

test("production snapshot server authenticates loopback requests", async () => {
  const server = await startSnapshotServer({ root: "/nonexistent/path/that/does/not/exist", port: 0, authToken: "test-token" });
  const url = `http://127.0.0.1:${server.port}/snapshot`;
  try {
    const noAuth = await fetch(url);
    assert.equal(noAuth.status, 401);
    const withAuth = await fetch(url, { headers: { Authorization: "Bearer test-token" } });
    assert.equal(withAuth.status, 200);
    const body = await withAuth.json();
    assert.equal(body.schemaVersion, SNAPSHOT_SCHEMA_VERSION);
    assert.equal(body.productId, "blueprint");
    assert.ok(Number.isInteger(body.observedAtUnixMs));
  } finally {
    await server.close();
  }
});

test("snapshot validates against pinned released schema", async () => {
  const { readFileSync } = await import("node:fs");
  const { default: Ajv } = await import("ajv");
  const validate = new Ajv().compile(JSON.parse(readFileSync("schemas/orthic-product-snapshot-v2.schema.json", "utf8")));
  const snapshot = buildSnapshot({ root: "/nonexistent/path/that/does/not/exist" });
  assert.equal(validate(snapshot), true, JSON.stringify(validate.errors));
});

test("dogfood check: no productId branching", async () => {
  const { readFileSync } = await import("node:fs");
  const manifestSrc = readFileSync("src/lib/init/manifest.mjs", "utf8");
  const snapSrc = readFileSync("src/lib/orthic-snapshot.mjs", "utf8");
  // No product-conditional branching in code meant to be product-generic (SEAM Unit 3c)
  assert.equal(/productId\s*===/.test(manifestSrc), false, "manifest must not branch on productId");
  assert.equal(/productId\s*===/.test(snapSrc), false, "snapshot must not branch on productId");
});
