import { dispatchHookEvent, normalizeHookEvent } from "@rightkit/hooks";

const HOOK_MODULES = Object.freeze([
  ["membrane.crypt-status", "SessionStart", "status"],
  ["membrane.memory-rearm", "SessionStart", "rearm", (event) => event.payload.source === "compact"],
  ["membrane.memory-recall", "UserPromptSubmit", "recall"],
  ["membrane.memory-pre-compact", "PreCompact", "preCompact"],
  ["membrane.memory-post-compact", "PostCompact", "postCompact"],
  ["membrane.memory-bump", "PreToolUse", "bump", (event) => event.payload.tool_name === "Read"],
  ["membrane.memory-conflict", "PreToolUse", "conflict", (event) => event.payload.tool_name === "Write"],
  ["membrane.tool-observer", "PostToolUse", "observe", (event) => event.payload.tool_name === "Bash"],
  ["membrane.memory-ingest", "PostToolUse", "ingest", (event) => ["Write", "Edit", "MultiEdit", "apply_patch"].includes(event.payload.tool_name)],
  ["membrane.memory-nag", "Stop", "nag"],
]);

function typedStatus(state, reason, detail = null) {
  return Object.freeze({ schemaVersion: 1, kind: "membrane.hook.status", state, reason, detail });
}

function requireOperation(operations, name) {
  const operation = operations?.[name];
  if (typeof operation !== "function") throw new TypeError(`Membrane hook operation is required: ${name}`);
  return operation;
}

/**
 * Membrane owns memory-hook policy; HookHost owns event normalization, ordering,
 * deadlines, and result envelopes. Lifecycle management is intentionally absent:
 * the Hub is the sole owner of Crypt start, restart, shutdown, and singleton
 * ownership.
 */
export function createMembraneHookModules(operations) {
  return Object.freeze(HOOK_MODULES.map(([id, hookEvent, operation, matches = () => true]) => Object.freeze({
    id,
    async handle(event, context) {
      if (event.event !== hookEvent || !matches(event)) return typedStatus("skipped", "event_not_applicable");
      return requireOperation(operations, operation)(event, context);
    },
  })));
}

/** Dispatch one host event through the neutral @rightkit/hooks runtime. */
export async function dispatchMembraneHookEvent(payload, operations, options = {}) {
  return dispatchHookEvent(payload, createMembraneHookModules(operations), options);
}

export { normalizeHookEvent, typedStatus };
