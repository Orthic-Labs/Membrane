import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const registry = JSON.parse(readFileSync(join(process.cwd(), "schemas/registry/toolsets.yaml"), "utf8"));

test("native registry declares conservative toolset groups", () => {
  assert.equal(registry.version, "membrane.toolsets.v1");
  assert.ok(Array.isArray(registry.groups.default));
  assert.ok(registry.groups.default.includes("membrane_context"));
  assert.ok(registry.groups.memory.includes("membrane_memory_read"));
  assert.ok(registry.groups.push.includes("membrane_push_prepare"));
  assert.ok(registry.groups.operator.includes("membrane_knowledge_review"));
});

test("native MCP implementation owns toolset negotiation", () => {
  const source = readFileSync(join(process.cwd(), "engine/crates/membrane-mcp/src/tools.rs"), "utf8");
  assert.match(source, /membrane\.toolsets\.v1/);
  assert.match(source, /CORE|ADAPT|OPERATOR|DIAGNOSTIC/);
});
