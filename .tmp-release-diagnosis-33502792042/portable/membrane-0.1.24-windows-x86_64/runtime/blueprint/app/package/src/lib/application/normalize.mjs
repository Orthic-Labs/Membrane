// Input normalization for the shared application service. Keeps query
// adaptation in one place so CLI, MCP, SDK, and hooks all feed the service
// the same canonical shapes.

import { fail } from "./errors.mjs";

export function clampInt(value, { min, max, fallback }) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(parsed)));
}

export function normalizeQueryInput(input = {}, defaults = {}) {
  const query = String(input.query ?? input.task ?? "").trim();
  if (!query) fail("query_required", "A non-empty query is required.");
  return {
    query,
    limit: clampInt(input.limit, { min: 1, max: defaults.maxLimit ?? 100, fallback: defaults.limit ?? 20 }),
    anchors: Array.isArray(input.anchors) ? input.anchors.map(String) : [],
  };
}

export function normalizeAnchorInput(input = {}, defaults = {}) {
  const anchor = String(input.anchor ?? "").trim();
  if (!anchor) fail("anchor_required", "An anchor is required.");
  return {
    anchor,
    depth: clampInt(input.depth, { min: 1, max: defaults.maxDepth ?? 8, fallback: defaults.depth ?? 1 }),
    budget: clampInt(input.budget, { min: 128, max: defaults.maxBudget ?? 32000, fallback: defaults.budget ?? 2000 }),
    direction: input.direction ?? "both",
    cursor: input.cursor ? String(input.cursor) : undefined,
  };
}
