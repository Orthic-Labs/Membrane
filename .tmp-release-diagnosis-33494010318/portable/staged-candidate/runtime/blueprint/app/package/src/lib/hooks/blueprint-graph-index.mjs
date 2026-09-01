import { dispatchHookEvent } from "@rightkit/hooks";

const INDEX_EVENTS = new Set(["PostToolUse", "Stop", "SessionEnd"]);

export function createBlueprintGraphIndexHook({ index, id = "blueprint-graph-index" } = {}) {
  if (typeof index !== "function") throw new TypeError("Blueprint graph-index hook requires index(event, context)");
  return Object.freeze({ id, async handle(event, context) {
    if (!INDEX_EVENTS.has(event.event)) return Object.freeze({ kind: "blueprint_graph_index", status: "skipped", reason: "event_ignored" });
    return Object.freeze({ kind: "blueprint_graph_index", status: "indexed", result: (await index(event, context)) ?? null });
  } });
}

// Blueprint owns graph/index semantics; RightKit supplies neutral normalisation,
// ordered dispatch, deadlines, and typed result envelopes.
export async function dispatchBlueprintGraphIndexHook(payload, { index, modules = [], timeoutMs = 3000 } = {}) {
  return dispatchHookEvent(payload, [createBlueprintGraphIndexHook({ index }), ...modules], { timeoutMs });
}
