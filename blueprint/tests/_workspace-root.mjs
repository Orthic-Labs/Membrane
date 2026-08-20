import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

/**
 * Walk upward for the parent monorepo that owns shared context contracts.
 * Standalone Cortex checkouts do not have this tree — use `required: false`
 * and skip workspace-only tests instead of failing a clean package test run.
 */
export function findWorkspaceRoot(start, { required = true } = {}) {
  let current = resolve(start);
  while (true) {
    if (existsSync(join(current, "tools/lib/context-contracts.schema.json"))) return current;
    const parent = dirname(current);
    if (parent === current) {
      if (!required) return null;
      throw new Error(`workspace contract root not found from ${start}`);
    }
    current = parent;
  }
}
