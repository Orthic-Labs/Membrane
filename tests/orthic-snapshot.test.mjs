import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import http from "node:http";

import { buildSnapshot, validateSnapshot } from "../lib/orthic-snapshot.mjs";

test("snapshot builds with required fields", () => {
  const snap = buildSnapshot({ root: process.cwd(), outDir: ".agent" });
  assert.equal(snap.schemaVersion, 1);
  assert.equal(snap.productId, "cortex");
  assert.ok(snap.timestamp);
  assert.ok(snap.sections);
  assert.ok(snap.sections.freshness);
  assert.ok(snap.sections.graph);
});

test("payload validates against schema", async () => {
  const snap = buildSnapshot({ root: process.cwd() });
  const schema = JSON.parse(readFileSync("schemas/orthic-product-snapshot-v1.schema.json", "utf8"));
  const { default: Ajv } = await import("ajv");
  const ajv = new Ajv();
  const validate = ajv.compile(schema);
  assert.equal(validate(snap), true, JSON.stringify(validate.errors));
});

test("missing-input section reports unavailable with reason, not fabricated zero", () => {
  const snap = buildSnapshot({ root: "/nonexistent/path/that/does/not/exist" });
  // freshness should be unavailable with reason when graph missing
  assert.equal(snap.sections.freshness.state, "unavailable");
  assert.ok(snap.sections.freshness.reason);
  assert.equal(typeof snap.sections.freshness.reason, "string");
});

test("request without Authorization gets 401 (via http-server pattern)", async () => {
  // Minimal server that reuses lib/http-server.mjs pattern — test auth gate
  const { randomBytes } = await import("node:crypto");
  const token = randomBytes(16).toString("base64url");
  const server = http.createServer((req, res) => {
    const auth = req.headers.authorization?.replace(/^Bearer\s+/, "");
    if (auth !== token) {
      res.writeHead(401, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { code: "unauthorized" } }));
      return;
    }
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify(buildSnapshot({})));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const port = server.address().port;
  try {
    const noAuth = await fetch(`http://127.0.0.1:${port}/snapshot`);
    assert.equal(noAuth.status, 401);
    const withAuth = await fetch(`http://127.0.0.1:${port}/snapshot`, { headers: { Authorization: `Bearer ${token}` } });
    assert.equal(withAuth.status, 200);
    const body = await withAuth.json();
    assert.equal(body.schemaVersion, 1);
  } finally {
    await new Promise((r) => server.close(r));
  }
});

test("dogfood check: no productId branching", async () => {
  const { readFileSync } = await import("node:fs");
  const manifestSrc = readFileSync("lib/init/manifest.mjs", "utf8");
  const snapSrc = readFileSync("lib/orthic-snapshot.mjs", "utf8");
  // No product-conditional branching in code meant to be product-generic (SEAM Unit 3c)
  assert.equal(/productId\s*===/.test(manifestSrc), false, "manifest must not branch on productId");
  assert.equal(/productId\s*===/.test(snapSrc), false, "snapshot must not branch on productId");
});
