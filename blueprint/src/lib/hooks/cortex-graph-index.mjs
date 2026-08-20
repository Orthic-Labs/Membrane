import { dispatchHookEvent } from "@rightkit/hooks";

const INDEX_EVENTS = new Set(["PostToolUse", "Stop", "SessionEnd"]);

export function createCortexGraphIndexHook({ index, id = "cortex-graph-index" } = {}) {
  if (typeof index !== "function") throw new TypeError("Cortex graph-index hook requires index(event, context)");
  return Object.freeze({ id, async handle(event, context) {
    if (!INDEX_EVENTS.has(event.event)) return Object.freeze({ kind: "cortex_graph_index", status: "skipped", reason: "event_ignored" });
    return Object.freeze({ kind: "cortex_graph_index", status: "indexed", result: (await index(event, context)) ?? null });
  } });
}

// Cortex owns graph/index semantics; RightKit supplies neutral normalisation,
// ordered dispatch, deadlines, and typed result envelopes.
export async function dispatchCortexGraphIndexHook(payload, { index, modules = [], timeoutMs = 3000 } = {}) {
  return dispatchHookEvent(payload, [createCortexGraphIndexHook({ index }), ...modules], { timeoutMs });
}
