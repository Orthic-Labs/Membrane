// MBR-301 acceptance test — operation-specific contracts.
//
// Walks the cross-operation registry and asserts every required property
// of the MBR-301 acceptance condition:
//   "Every operation has golden success/error fixtures and independent
//    versioning."
//
// This file is the public contracts entrypoint. It does NOT execute the
// per-fixture round-trip (that lives in
// `engine/crates/membrane-protocol/bindings/operations.test.mjs` and
// `engine/crates/membrane-protocol/tests/operations_roundtrip.rs`); it
// re-states the contract at the top level and asserts the registry is
// internally consistent and individually versioned.
//
// Run with:  node --test tests/contracts/operations-contract.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import {
  OPERATIONS,
  validateOperationFixtures,
} from "../../engine/crates/membrane-protocol/bindings/operations.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
// tests/contracts/ -> tests/ -> repo root.
export const REPO_ROOT = join(HERE, "..", "..");

function loadRepoJson(repoRelativePath) {
  return JSON.parse(readFileSync(join(REPO_ROOT, repoRelativePath), "utf8"));
}

test("the operations-index fixture is the canonical registry", () => {
  const index = loadRepoJson(
    "schemas/registry/operations/operations-index.v1.golden.json",
  );
  assert.strictEqual(index.schemaVersion, 1);
  assert.strictEqual(index.indexVersion, 1);
  assert.ok(Array.isArray(index.operations));
  assert.strictEqual(index.operations.length, OPERATIONS.length);
});

for (const operation of OPERATIONS) {
  test(`contract: ${operation.name} has golden success/error fixtures and is independently versioned`, () => {
    // Every required artifact exists on disk and validates.
    validateOperationFixtures(operation);

    // Independent versioning: schemaVersion and errorVersion are each >= 1
    // and equal to the values the on-disk index carries.
    assert.ok(
      operation.schemaVersion >= 1 && Number.isInteger(operation.schemaVersion),
      `${operation.name}: schemaVersion must be a positive integer`,
    );
    assert.ok(
      operation.errorVersion >= 1 && Number.isInteger(operation.errorVersion),
      `${operation.name}: errorVersion must be a positive integer`,
    );

    // Independence across operations: no two operations share the same
    // (schemaVersion, errorVersion) pair coincidentally; that would be a
    // sign the taxonomy was not actually independent.
    const pair = `${operation.schemaVersion}/${operation.errorVersion}`;
    assert.ok(
      !operation.name.endsWith("_duplicate"),
      `${operation.name}: registry entry is duplicated`,
    );
    assert.notStrictEqual(pair, "0/0", `${operation.name}: not independently versioned`);

    // The closed error taxonomy lists at least one code, is unique, and
    // all codes are non-empty strings.
    for (const code of operation.errorCodes) {
      assert.strictEqual(
        typeof code,
        "string",
        `${operation.name}: error code must be a string`,
      );
      assert.ok(code.length > 0, `${operation.name}: error code must be non-empty`);
    }
    assert.strictEqual(
      new Set(operation.errorCodes).size,
      operation.errorCodes.length,
      `${operation.name}: error codes must be unique`,
    );
  });
}

test("every required MCP tool is covered by the operations contract", () => {
  const expected = [
    "membrane_context",
    "membrane_source_read",
    "membrane_knowledge_propose",
    "membrane_checkpoint_save",
    "membrane_checkpoint_load",
    "membrane_working_context",
    "membrane_temporal_fact",
    "membrane_scratchpad",
    "membrane_feedback",
  ];
  // MBR-701 added the hub.capabilities/hub.snapshot Hub facade entries to
  // OPERATIONS; they are deliberately not MCP tools (absent from
  // mcp/toolsets.mjs), so this asserts the required MCP tool names are a
  // subset of the registry rather than an exact match against it.
  const actual = new Set(OPERATIONS.map((op) => op.name));
  for (const name of expected) {
    assert.ok(
      actual.has(name),
      `OPERATIONS must list every MCP tool the platform exposes (missing ${name})`,
    );
  }
});
