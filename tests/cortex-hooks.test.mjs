import assert from "node:assert/strict";
import test from "node:test";
import { dispatchCortexGraphIndexHook } from "../lib/hooks/cortex-graph-index.mjs";

test("Cortex graph/index hook uses neutral normalized ordered dispatch with typed results", async () => {
  const order = [];
  const result = await dispatchCortexGraphIndexHook({ hook_event_name: "PostToolUse", thread_id: "t-1" }, {
    index: async (event, { signal }) => { order.push(`index:${event.sessionId}`); assert.equal(signal.aborted, false); return { generationId: "g-1" }; },
    modules: [{ id: "after", handle: () => order.push("after") }],
  });
  assert.deepEqual(order, ["index:t-1", "after"]);
  assert.equal(result.status, "ok");
  assert.deepEqual(result.results[0].output, { kind: "cortex_graph_index", status: "indexed", result: { generationId: "g-1" } });
});

test("Cortex graph/index hook preserves runtime deadline result", async () => {
  const result = await dispatchCortexGraphIndexHook({ event: "Stop" }, { index: () => new Promise(() => {}), timeoutMs: 5 });
  assert.equal(result.status, "error");
  assert.match(result.results[0].error, /timed out/);
});
