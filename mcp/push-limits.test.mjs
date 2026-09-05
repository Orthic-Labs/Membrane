import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { MAX_PUSH_REQUEST_BYTES } from "./push-limits.mjs";

test("push ingress accommodates the complete advertised one-megachar text contract", async () => {
  const tools = JSON.parse(await readFile(new URL("../schemas/registry/push-tools.v1.json", import.meta.url), "utf8"));
  const prepare = tools.find((tool) => tool.name === "membrane_push_prepare");
  const maxChars = prepare.inputSchema.properties.request.properties.text.maxLength;
  assert.equal(maxChars, 1_048_576);
  // NUL is one schema character but JSON.stringify expands it to six wire bytes (\\u0000).
  const body = { repository: "r", caller: { root: "/r", repositoryId: "r", scopeId: "s" }, request: { text: "\u0000".repeat(maxChars), maxBytes: 1 } };
  const bytes = Buffer.byteLength(JSON.stringify(body), "utf8");
  assert.ok(bytes > 32 * 1024);
  assert.ok(bytes <= MAX_PUSH_REQUEST_BYTES, `${bytes} exceeds Push transport bound`);
});
