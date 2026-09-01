#!/usr/bin/env node
// MBR-206 — Client registry contract test.
//
// Book mode: this test is committed in the MBR-206 commit but is NOT executed
// at task time. It runs at the Book 1 gate along with every other deferred
// command.
//
// Run:  node tests/clients/client-matrix.test.mjs
// Exit: 0 when all assertions pass, 1 otherwise.
//
// Asserts:
//   (a) every declared client can be discovered through at least one transport,
//   (b) every supported_operation reference resolves to a real operation in
//       schemas/operations/operations/,
//   (c) the generated matrix is byte-stable across runs.
//
// The test imports the same pure builder functions the generator uses so the
// assertions are pinned to the published contract, not to disk artifacts.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  buildArtifacts,
  parseYaml,
  stableStringify,
  writeArtifacts,
} from "../../scripts/tools/productization/generate-client-matrix.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = join(HERE, "..", "..");

const REGISTRY_TEXT = readFileSync(join(REPO_ROOT, "schemas", "registry", "clients.yaml"), "utf8");
const OPERATION_INDEX = JSON.parse(readFileSync(join(REPO_ROOT, "schemas", "registry", "operations", "operations-index.v1.golden.json"), "utf8"));
const REGISTRY = parseYaml(REGISTRY_TEXT);

const checks = [];
function test(name, fn) {
  checks.push([name, fn]);
}

test("registry declares the six canonical clients", () => {
  const ids = REGISTRY.clients.map((client) => client.id).sort();
  assert.deepEqual(ids, ["antigravity", "claude", "codex", "cursor", "generic_mcp", "windsurf"]);
});

test("every client has a non-empty, lowercase snake_case id", () => {
  const seen = new Set();
  for (const client of REGISTRY.clients) {
    assert.match(client.id, /^[a-z][a-z0-9_]*$/, `bad id: ${client.id}`);
    assert.ok(!seen.has(client.id), `duplicate id: ${client.id}`);
    seen.add(client.id);
  }
});

test("every client declares a transport and a discovery_method", () => {
  for (const client of REGISTRY.clients) {
    assert.ok(["stdio", "loopback"].includes(client.transport), `bad transport for ${client.id}: ${client.transport}`);
    assert.ok(typeof client.discovery_method === "string" && client.discovery_method.length > 0,
      `bad discovery_method for ${client.id}`);
  }
});

test("(a) every client is discoverable through at least one transport", () => {
  // Every client must have a non-empty install_command and either a binary_env
  // (so the install path can locate the binary) or a stdio transport with a
  // discoverable command. We treat "transport=stdio AND install_command non-empty"
  // as the canonical discoverable combination.
  for (const client of REGISTRY.clients) {
    const hasTransport = client.transport === "stdio" || client.transport === "loopback";
    assert.ok(hasTransport, `${client.id}: transport missing`);
    const hasInstallCommand = typeof client.install_command === "string" && client.install_command.length > 0;
    assert.ok(hasInstallCommand, `${client.id}: install_command missing`);
    // At least one of: explicit detect_command, binary_env, or a transport that
    // implies discovery.
    const hasDiscoveryHook =
      (typeof client.detect_command === "string" && client.detect_command.length > 0) ||
      (typeof client.binary_env === "string" && client.binary_env.length > 0) ||
      client.discovery_method === "global-mcp-config" ||
      client.transport === "loopback";
    assert.ok(hasDiscoveryHook, `${client.id}: no discovery hook (need command, config-file, or loopback discovery)`);
  }
});

test("(b) every supported_operations reference resolves to a real operation", () => {
  const knownOps = new Set(OPERATION_INDEX.operations.map((op) => op.name));
  for (const client of REGISTRY.clients) {
    for (const opName of client.supported_operations) {
      assert.ok(knownOps.has(opName), `${client.id} references unknown operation '${opName}'`);
    }
  }
});

test("(b) every degraded_operations reference resolves to a real operation", () => {
  const knownOps = new Set(OPERATION_INDEX.operations.map((op) => op.name));
  for (const client of REGISTRY.clients) {
    for (const opName of client.degraded_operations ?? []) {
      assert.ok(knownOps.has(opName), `${client.id} degraded reference unknown operation '${opName}'`);
    }
  }
});

test("no operation appears in both supported_operations and degraded_operations for the same client", () => {
  for (const client of REGISTRY.clients) {
    const supported = new Set(client.supported_operations);
    for (const opName of client.degraded_operations ?? []) {
      assert.ok(!supported.has(opName), `${client.id} lists '${opName}' in both supported and degraded`);
    }
  }
});

test("every client declares an authority_grant with max_level and loopback_only", () => {
  for (const client of REGISTRY.clients) {
    const grant = client.authority_grant;
    assert.ok(grant, `${client.id}: authority_grant missing`);
    assert.ok(["L0", "L1", "L2", "L3", "L4", "L5"].includes(grant.max_level),
      `${client.id}: bad max_level ${grant.max_level}`);
    assert.equal(typeof grant.loopback_only, "boolean", `${client.id}: loopback_only must be boolean`);
    assert.ok(Array.isArray(grant.scopes) && grant.scopes.length > 0,
      `${client.id}: authority_grant.scopes must be a non-empty array`);
  }
});

test("(c) the generated matrix is byte-stable across two runs", () => {
  const a = buildArtifacts(REGISTRY, OPERATION_INDEX);
  const b = buildArtifacts(REGISTRY, OPERATION_INDEX);
  assert.equal(stableStringify(a.capabilities), stableStringify(b.capabilities));
  assert.equal(stableStringify(a.matrix), stableStringify(b.matrix));
});

test("(c) the generated matrix has the cartesian product cells", () => {
  const { matrix } = buildArtifacts(REGISTRY, OPERATION_INDEX);
  const expectedCount = REGISTRY.clients.length * OPERATION_INDEX.operations.length;
  assert.equal(matrix.cells.length, expectedCount,
    `expected ${expectedCount} cells, got ${matrix.cells.length}`);
  // Every (client, operation) pair is present exactly once.
  const seen = new Set();
  for (const cell of matrix.cells) {
    const key = `${cell.client}::${cell.operation}`;
    assert.ok(!seen.has(key), `duplicate cell: ${key}`);
    seen.add(key);
    assert.ok(["supported", "degraded", "unsupported"].includes(cell.support), `bad support: ${cell.support}`);
  }
});

test("(c) the artifacts written to disk match the in-memory build", async () => {
  const { capabilities, matrix, capabilitiesOut, matrixOut } = await writeArtifacts();
  const capsOnDisk = JSON.parse(readFileSync(capabilitiesOut, "utf8"));
  const matrixOnDisk = JSON.parse(readFileSync(matrixOut, "utf8"));
  assert.equal(stableStringify(capsOnDisk), stableStringify(capabilities));
  assert.equal(stableStringify(matrixOnDisk), stableStringify(matrix));
});

test("matrix clients and operations arrays match the registry and the index", () => {
  const { matrix } = buildArtifacts(REGISTRY, OPERATION_INDEX);
  const matrixClientIds = matrix.clients.map((c) => c.id).sort();
  const registryIds = REGISTRY.clients.map((c) => c.id).sort();
  assert.deepEqual(matrixClientIds, registryIds);
  const matrixOpNames = matrix.operations.map((op) => op.name).sort();
  const indexOpNames = OPERATION_INDEX.operations.map((op) => op.name).sort();
  assert.deepEqual(matrixOpNames, indexOpNames);
});

test("the install path's read-only helpers see the same client list", async () => {
  const install = await import("../../mcp/install.mjs");
  const ids = await install.clientsForEnrollment();
  const expected = REGISTRY.clients.map((c) => c.id);
  assert.deepEqual(ids, expected);
  // supportedOperationsFor returns the matrix's 'supported' subset for each
  // client.
  for (const client of REGISTRY.clients) {
    const supported = await install.supportedOperationsFor(client.id);
    const expectedOps = client.supported_operations.slice().sort();
    assert.deepEqual(supported.slice().sort(), expectedOps, `mismatch for ${client.id}`);
  }
});

let failed = 0;
for (const [name, fn] of checks) {
  try {
    await fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL - ${name}: ${error.message}`);
  }
}
if (failed) {
  console.error(`client-matrix self-test: ${failed}/${checks.length} failed`);
  process.exit(1);
}
console.log(`client-matrix self-test: ${checks.length}/${checks.length} passed`);
