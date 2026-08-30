import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { join } from "node:path";

// MBR-308: mechanical drift check between docs/reference/sdk/http-transport.md (the
// wire-contract reference an SDK caller reads to build an HTTP transport
// function) and the real MBR-306 transport source plus its published
// schema. If a header name, the route, or a denial code changes in the
// source or schema without the doc being updated, this test fails instead
// of the drift being caught only by review.

const root = join(import.meta.dirname, "../..");
const read = (path) => readFile(join(root, path), "utf8");

function constString(source, constName) {
  const match = source.match(new RegExp(`pub const ${constName}: &str = "([^"]+)"`));
  assert.ok(match, `expected ${constName} in mcp_http.rs`);
  return match[1];
}

test("http-transport.md names the same path and headers as mcp_http.rs", async () => {
  const [source, doc] = await Promise.all([
    read("engine/crates/membrane-runtime/src/mcp_http.rs"),
    read("docs/reference/sdk/http-transport.md"),
  ]);
  const path = constString(source, "MCP_HTTP_PATH");
  const installationHeader = constString(source, "INSTALLATION_HEADER");
  const sessionHeader = constString(source, "SESSION_HEADER");
  for (const literal of [path, installationHeader, sessionHeader]) {
    assert.ok(doc.includes(literal), `docs/reference/sdk/http-transport.md must name ${literal}`);
  }
});

test("http-transport.md names every denial code the schema defines", async () => {
  const [schemaText, doc] = await Promise.all([
    read("schemas/http-mcp-security.v1.schema.json"),
    read("docs/reference/sdk/http-transport.md"),
  ]);
  const schema = JSON.parse(schemaText);
  const denialCodes = schema.properties.denial.enum;
  assert.ok(Array.isArray(denialCodes) && denialCodes.length > 0);
  for (const code of denialCodes) {
    assert.ok(doc.includes(code), `docs/reference/sdk/http-transport.md must name denial code ${code}`);
  }
});

test("the Rust client's own compatibility tests exist and read the shared golden fixtures", async () => {
  const rustTests = await read("engine/crates/membrane-client/tests/compat.rs");
  for (const fixture of [
    "membrane-context.v1.golden.json",
    "membrane-context.v1.error.golden.json",
    "membrane-source-read.v1.golden.json",
    "membrane-source-read.v1.error.golden.json",
    "operations-index.v1.golden.json",
  ]) {
    assert.ok(
      rustTests.includes(fixture),
      `engine/crates/membrane-client/tests/compat.rs must read the same shared fixture ${fixture} the TypeScript and Python suites use`,
    );
  }
});
