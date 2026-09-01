#!/usr/bin/env node
// Installed product hook entrypoint. HookHost owns normalization/order/deadlines.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { dispatchMembraneHookEvent } from "./membrane-hook-runtime.mjs";
import { createWorkspaceMemoryOperations } from "./membrane-workspace-operations.mjs";

function hostOutput(result) {
  const additionalContext = result.results
    .map(({ output }) => output?.detail?.additionalContext)
    .filter(Boolean)
    .join("\n\n");

  // Semantic Edit Fence enforcement (design §10): a "blocked" status at the
  // PreToolUse or Stop boundary becomes an actual host deny decision, so the
  // host cannot run tests/builds/completion across unclean bytes. This is
  // the enforcement half of "hosts enforce; providers report evidence".
  const blocked = result.results.find(({ output }) => output?.state === "blocked");
  if (blocked && (result.event === "PreToolUse" || result.event === "Stop")) {
    const reason = String(
      blocked.output?.detail?.detail
        || blocked.output?.reason
        || "semantic edit fence not cleared",
    );
    return Object.freeze({
      decision: "block",
      reason,
      hookSpecificOutput: {
        hookEventName: result.event,
        permissionDecision: "deny",
        permissionDecisionReason: reason,
        additionalContext,
      },
      membraneHook: result,
    });
  }

  return Object.freeze({
    hookSpecificOutput: {
      hookEventName: result.event,
      additionalContext,
    },
    membraneHook: result,
  });
}

export async function runHook(payload, { operations = createWorkspaceMemoryOperations(), timeoutMs = 3000 } = {}) {
  return hostOutput(await dispatchMembraneHookEvent(payload, operations, { timeoutMs }));
}

export async function main({ read = () => readFileSync(0, "utf8"), write = (value) => process.stdout.write(value), ...options } = {}) {
  let payload;
  try { payload = JSON.parse(read()); } catch { payload = {}; }
  write(`${JSON.stringify(await runHook(payload, options))}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
