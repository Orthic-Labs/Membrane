import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { prepareToolEgress } from "./push-tool-egress.mjs";
import { pushRequest } from "../push-client.mjs";
import { toolsetNames } from "../toolsets.mjs";
const hash = (s) => `sha256:${createHash("sha256").update(s).digest("hex")}`;
const text = "ordinary repeated context\n".repeat(1000);
const original = { content: [{ type: "text", text }], structuredContent: { data: { text }, trace: { traceparent: "trace" } }, isError: false, toolCallId: "call-one" };
const binding = { repository: "repo", caller: { root: "/repo", repositoryId: "repo", scopeId: "session" } };
const token = "a".repeat(64);
const delivery = () => ({ text: "compact", disposition: "prepared", representationKind: "protected_lines_v1",
  recovery: { handle: `mr://anchor/${"b".repeat(64)}`, expiresAt: 2000 },
  receipt: { sourceDigest: hash(text), representationDigest: hash("compact") } });

test("owned egress preserves envelope, measures actual delivery and does not echo original", async () => {
  let calls = 0;
  const out = await prepareToolEgress(original, binding, { resolverToken: token, maxBytes: 2048, request: async (operation, args) => {
    calls++; assert.equal(operation, "membrane_push_prepare"); assert.equal(args.request.text, text); return delivery();
  } });
  assert.equal(out.state, "prepared"); assert.equal(calls, 1);
  assert.equal(out.result.toolCallId, original.toolCallId); assert.equal(out.result.isError, false);
  assert.deepEqual(out.result.structuredContent.trace, original.structuredContent.trace);
  assert.equal(out.receipt.envelopeBytes, Buffer.byteLength(JSON.stringify(out.result)));
  assert.ok(!JSON.stringify(out.result).includes(text));
  assert.equal(original.content[0].text, text);
});
test("exact, mixed parts, missing resolver and invalid proof are not reduced", async () => {
  const shouldNotCall = async () => { throw new Error("must not call"); };
  for (const result of [{ ...original, disposition: "exact" }, { ...original, isError: true }, { ...original, content: [{ type: "image", data: "x" }] }]) {
    const out = await prepareToolEgress(result, binding, { resolverToken: token, maxBytes: 2048, request: shouldNotCall });
    assert.equal(out.result, result);
  }
  const out = await prepareToolEgress(original, binding, { resolverToken: token, maxBytes: 2048, request: async () => ({ ...delivery(), receipt: {} }) });
  assert.equal(out.receipt.reason, "delivery_identity_mismatch"); assert.equal(out.result, original);
});
test("Push client preserves typed refusal and only targets loopback", async () => {
  await assert.rejects(pushRequest("membrane_push_resolve", binding, { request: async (input) => {
    assert.equal(input.host, "127.0.0.1"); assert.equal(input.path, "/push/resolve");
    return { status: 410, body: JSON.stringify({ result: { kind: "error", code: "push_artifact_expired" } }) };
  } }), /push_artifact_expired/);
});
test("Push is discoverable by explicit toolset without widening the default", () => {
  assert.deepEqual(toolsetNames(), ["membrane_context", "membrane_source_read", "membrane_ledger"]);
  assert.deepEqual(toolsetNames({ _meta: { "membrane.toolsets.v1": ["push"] } }), ["membrane_context", "membrane_source_read", "membrane_ledger", "membrane_push_prepare", "membrane_push_resolve"]);
});
