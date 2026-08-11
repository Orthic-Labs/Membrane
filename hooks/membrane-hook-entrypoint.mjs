#!/usr/bin/env node
// Installed product hook entrypoint. HookHost owns normalization/order/deadlines.
import { readFileSync } from "node:fs";
import { dispatchMembraneHookEvent } from "./membrane-hook-runtime.mjs";
import { createWorkspaceMemoryOperations } from "./membrane-workspace-operations.mjs";

function hostOutput(result) {
  const additionalContext = result.results
    .map(({ output }) => output?.detail?.additionalContext)
    .filter(Boolean)
    .join("\n\n");
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

if (import.meta.url === `file://${process.argv[1]}`) main();
