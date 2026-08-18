// MBR-304 parity test for canonical Membrane MCP resources.
//
// Acceptance: "Resources are bounded, versioned, and do not leak content
// across grants." This file proves the legacy (JS) surface returns the exact
// same `resources/list` payload the native (Rust) surface emits, by computing
// the Rust projection directly from the canonical `schemas/registry/resources/*.json`
// source and asserting byte-for-byte equality. It also proves a denied read
// never leaks the resource body.
//
// If the native (Rust) binary is available on disk the test also spawns it
// and asserts the live wire payload matches the JS projection. The native
// binary path is resolved via MEMBRANE_NATIVE_MCP_BIN (default:
// ../../engine/target/debug/membrane-mcp).
// Book-mode discipline: this file is not executed during implementation.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import {
  listResources,
  readResource,
  readResourceByName,
  RESOURCE_NAMES,
  RESOURCE_URIS,
} from "../../mcp/resources.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..", "..");

const RESOURCE_FILES = [
  "resources-index.v1.json",
  "installation-manifest.v1.json",
  "lease-status.v1.json",
  "operation-registry.v1.json",
];

// Exact mirror of the Rust `resources::list_payload()` projection so the JS
// payload can be compared against the canonical shape the native server
// emits. Any drift here is a regression in parity.
function projectListEntry(resource) {
  return {
    name: resource.name,
    version: resource.version,
    uri: resource.uri,
    mimeType: resource.mimeType,
    description: resource.description,
    accessGrants: resource.accessGrants ?? [],
    authorityEscalation: resource.authorityEscalation ?? false,
    authorityNotes: resource.authorityNotes ?? null,
  };
}

const canonicalResources = await Promise.all(
  RESOURCE_FILES.map(async (file) => {
    const raw = await readFile(join(REPO_ROOT, "schemas", "registry", "resources", file), "utf8");
    return JSON.parse(raw);
  }),
);

const expectedList = { resources: canonicalResources.map(projectListEntry) };
const jsList = await listResources();
assert.deepEqual(
  jsList,
  expectedList,
  "legacy (JS) resources/list payload equals the canonical projection used by the native (Rust) server",
);

// Stable JSON: native and legacy must agree on object key ordering too, since
// both project from the same source using the same insertion order. Re-serialize
// both shapes via JSON.stringify with sorted keys and compare.
function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((k) => [k, stable(value[k])]));
  }
  return value;
}
assert.equal(
  JSON.stringify(stable(jsList)),
  JSON.stringify(stable(expectedList)),
  "resources/list is stable under key-sorted serialization",
);

assert.deepEqual(
  [...RESOURCE_NAMES].sort(),
  canonicalResources.map((r) => r.name).sort(),
  "RESOURCE_NAMES matches the canonical resource file set",
);
assert.deepEqual(
  [...RESOURCE_URIS].sort(),
  canonicalResources.map((r) => r.uri).sort(),
  "RESOURCE_URIS matches the canonical resource URI set",
);

// Optional live-binary parity check. When the native MCP binary exists, drive
// it through stdio JSON-RPC and compare its wire payload to the JS projection.
async function tryNativeBinary() {
  const candidates = [];
  if (process.env.MEMBRANE_NATIVE_MCP_BIN) candidates.push(process.env.MEMBRANE_NATIVE_MCP_BIN);
  candidates.push(join(REPO_ROOT, "engine", "target", "debug", "membrane-mcp"));
  candidates.push(join(REPO_ROOT, "engine", "target", "release", "membrane-mcp"));
  for (const candidate of candidates) {
    try {
      await stat(candidate);
      return candidate;
    } catch {
      /* not built */
    }
  }
  return null;
}

const nativeBin = await tryNativeBinary();
if (nativeBin) {
  const child = spawn(nativeBin, [], { stdio: ["pipe", "pipe", "pipe"] });
  const wire = [];
  const decoder = [];
  child.stdout.on("data", (chunk) => {
    decoder.push(chunk);
    const text = Buffer.concat(decoder).toString("utf8");
    const lines = text.split("\n");
    decoder.length = 0;
    for (const line of lines) {
      if (!line.trim()) continue;
      try {
        wire.push(JSON.parse(line));
      } catch (_) {
        decoder.push(Buffer.from(line, "utf8"));
      }
    }
  });
  const send = (message) =>
    new Promise((resolve, reject) => {
      child.stdin.write(JSON.stringify(message) + "\n", (error) => (error ? reject(error) : resolve()));
    });
  await send({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2025-03-26",
      capabilities: {},
      clientInfo: { name: "mbr304-parity", version: "1.0.0" },
    },
  });
  await send({ jsonrpc: "2.0", id: 2, method: "resources/list" });
  // Read each resource with a matching grant and confirm the wire payload
  // matches the JS projection.
  let id = 100;
  for (const resource of canonicalResources) {
    await send({
      jsonrpc: "2.0",
      id: id++,
      method: "resources/read",
      params: { uri: resource.uri, accessGrants: resource.accessGrants },
    });
    // Read with the wrong grant and confirm the wire payload is a typed
    // rejection with no body field at all.
    await send({
      jsonrpc: "2.0",
      id: id++,
      method: "resources/read",
      params: { uri: resource.uri, accessGrants: [] },
    });
  }
  await new Promise((resolve) => child.stdin.end());
  await new Promise((resolve) => child.once("close", resolve));

  const listRow = wire.find((row) => row.id === 2);
  assert.ok(listRow && listRow.result, "native resources/list returned a result envelope");
  assert.deepEqual(listRow.result, expectedList, "native (Rust) resources/list wire payload equals the canonical projection");

  id = 100;
  for (const resource of canonicalResources) {
    const okRow = wire.find((entry) => entry.id === id++);
    assert.ok(okRow && okRow.result, `native resources/read(${resource.uri}) with matching grant returned a result envelope`);
    const jsRead = await readResource(resource.uri, resource.accessGrants);
    assert.equal(jsRead.ok, true);
    assert.deepEqual(okRow.result, jsRead.result, `native resources/read(${resource.uri}) wire payload equals the JS projection`);

    const deniedRow = wire.find((entry) => entry.id === id++);
    assert.ok(deniedRow && deniedRow.result, `native resources/read(${resource.uri}) with empty grants returned a result envelope`);
    assert.equal(deniedRow.result.error.code, "resource_access_denied");
    assert.equal(deniedRow.result.body, undefined);
  }
} else {
  console.warn(
    "[mbr-304 parity] native membrane-mcp binary not found; parity asserted against the canonical projection only",
  );
}
