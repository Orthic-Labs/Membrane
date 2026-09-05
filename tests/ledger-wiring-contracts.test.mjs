import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8");

test("Ledger is discoverable through native and JS MCP surfaces without displacing Push", async () => {
  const [rustTools, native, protocol, jsToolsets, registry, server, executor, cli, qualification] = await Promise.all([
    read("engine/crates/membrane-mcp/src/tools.rs"),
    read("engine/crates/membrane-runtime/src/pull/native_federation.rs"),
    read("engine/crates/membrane-protocol/src/federation.rs"),
    read("mcp/toolsets.mjs"),
    read("schemas/registry/toolsets.yaml"),
    read("mcp/server.mjs"),
    read("engine/crates/membrane-runtime/src/mcp_executor.rs"),
    read("engine/crates/membrane-runtime/src/ledger/cli.rs"),
    read("engine/crates/membrane-runtime/src/ledger/qualification.rs"),
  ]);
  assert.match(protocol, /Ledger/);
  assert.match(native, /ProviderId::Ledger/);
  assert.match(rustTools, /membrane_context", "membrane_source_read", "membrane_ledger/);
  assert.match(rustTools, /"push" => &CORE\[11\.\.\]/);
  assert.match(jsToolsets, /membrane_context,membrane_source_read,membrane_ledger/);
  const groups = JSON.parse(registry).groups;
  assert.deepEqual(groups.default, ["membrane_context", "membrane_source_read", "membrane_ledger"]);
  assert.deepEqual(groups.ledger, ["membrane_source_read", "membrane_ledger"]);
  assert.deepEqual(groups.push, ["membrane_push_prepare", "membrane_push_resolve"]);
  assert.match(server, /name: "membrane_ledger"/);
  assert.match(server, /\["ledger", "read", "--repo", binding\.root/);
  assert.match(executor, /"membrane_source_read" \| "membrane_ledger"/);
  assert.doesNotMatch(cli, /LedgerDb::open_default/);
  assert.match(qualification, /const QUALIFIED_DELIVERIES: &\[QualifiedDelivery\] = &\[\];/);
});
