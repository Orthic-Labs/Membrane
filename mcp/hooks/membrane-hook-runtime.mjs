import { dispatchHookEvent, normalizeHookEvent } from "@rightkit/hooks";
import { isVerificationCommand } from "../lib/verification-command.mjs";

/** Whether one PreToolUse event addresses a test/build/completion boundary.
 *
 * Delegates to the single verification-command classifier so hook runtime
 * and workspace operations never diverge (design §10, §11).
 */
export function isFenceRelevantCommand(event) {
  const tool = String(event.payload.tool_name || event.payload.toolName || "").toLowerCase();
  if (!["bash", "shell", "terminal", "command", "task"].includes(tool)) return false;
  const command = String(
    event.payload.tool_input?.command
      ?? event.payload.tool_input?.cmd
      ?? event.payload.command
      ?? "",
  );
  return isVerificationCommand(command, "");
}

const HOOK_MODULES = Object.freeze([
  ["membrane.cortex-status", "SessionStart", "status"],
  ["membrane.memory-rearm", "SessionStart", "rearm", (event) => event.payload.source === "compact"],
  ["membrane.memory-recall", "UserPromptSubmit", "recall"],
  ["membrane.memory-pre-compact", "PreCompact", "preCompact"],
  ["membrane.memory-post-compact", "PostCompact", "postCompact"],
  ["membrane.memory-bump", "PreToolUse", "bump", (event) => event.payload.tool_name === "Read"],
  ["membrane.diagnostics-fence", "PreToolUse", "enforceFence", isFenceRelevantCommand],
  ["membrane.memory-conflict", "PreToolUse", "conflict", (event) => event.payload.tool_name === "Write"],
  ["membrane.tool-observer", "PostToolUse", "observe", (event) => event.payload.tool_name === "Bash"],
  ["membrane.memory-ingest", "PostToolUse", "ingest", (event) => ["Write", "Edit", "MultiEdit", "apply_patch"].includes(event.payload.tool_name)],
  ["membrane.diagnostics-observe", "PostToolUse", "observeMutation", (event) => ["Write", "Edit", "MultiEdit", "apply_patch"].includes(event.payload.tool_name)],
  // Completion boundary: a Stop that would end the session on unclean bytes
  // is blocked exactly like an escalating test/build command (design §10).
  ["membrane.diagnostics-completion-fence", "Stop", "enforceCompletion"],
  ["membrane.memory-nag", "Stop", "nag"],
  ["membrane.memory-failure", "PostToolUseFailure", "postToolUseFailure"],
  ["membrane.memory-episode", "TaskCompleted", "taskCompleted"],
  ["membrane.memory-session-end", "SessionEnd", "sessionEnd"],
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
 * the Hub is the sole owner of Cortex start, restart, shutdown, and singleton
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
