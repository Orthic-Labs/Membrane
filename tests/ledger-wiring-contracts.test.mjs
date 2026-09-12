import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8");

test("Ledger feeds Pull while public registry remains Pull and memory Push", async () => {
  const [rustTools, native, protocol, nativeMatrix, registry, executor, cli, qualification] = await Promise.all([
    read("engine/crates/membrane-mcp/src/tools.rs"),
    read("engine/crates/membrane-runtime/src/pull/native_federation.rs"),
    read("engine/crates/membrane-protocol/src/federation.rs"),
    read("engine/crates/membrane-mcp/fixtures/capability-matrix.v1.json"),
    read("schemas/registry/toolsets.yaml"),
    read("engine/crates/membrane-runtime/src/mcp_executor.rs"),
    read("engine/crates/membrane-runtime/src/ledger/cli.rs"),
    read("engine/crates/membrane-runtime/src/ledger/qualification.rs"),
  ]);
  assert.match(protocol, /Ledger/);
  assert.match(native, /ProviderId::Ledger/);
  const publicRegistry = rustTools.match(/const PUBLIC: &[^;]+;/)?.[0] ?? "";
  assert.equal(publicRegistry, 'const PUBLIC: &[&str] = &["pull", "push"];');
  assert.equal(JSON.parse(nativeMatrix).schema, "membrane.host-capability-matrix.v1");
  const groups = JSON.parse(registry).groups;
  assert.deepEqual(groups.default, [
    "membrane_context", "membrane_source_read", "membrane_blueprint",
    "membrane_knowledge_propose", "membrane_memory", "membrane_memory_read",
    "membrane_checkpoint_save", "membrane_checkpoint_load",
  ]);
  assert.equal(groups.ledger, undefined);
  assert.deepEqual(groups.push, ["membrane_push_prepare", "membrane_push_resolve"]);
  assert.match(executor, /"membrane_source_read" \| "membrane_ledger"/);
  assert.doesNotMatch(cli, /LedgerDb::open_default/);
  assert.match(qualification, /const QUALIFIED_DELIVERIES: &\[QualifiedDelivery\] = &\[\];/);
});
