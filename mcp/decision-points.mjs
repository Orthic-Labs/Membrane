// Decision-point routing for discovery operations (plan 2.6).
//
// The passive instruction "use membrane_context" demonstrably fails: an agent
// mid-task reaches for grep because grep is right there. This module decides,
// per tool call, whether a cheaper and more current answer exists — and says so
// at the moment the agent is about to search, not at session start.
//
// Design constraints from the plan:
//   - exact-file reads and greps PASS untouched (they are already precise)
//   - broad discovery greps get a `membrane_context` suggestion
//   - cross-repo discovery routes through Membrane
//   - a Membrane miss allows the fallback and RECORDS an omission
//
// This never blocks. A suggestion that stops work would be worse than the
// problem it solves; the routing is advisory and always yields a decision the
// caller can ignore.

// The routing logic lives in a CommonJS module so the Sentinel PreToolUse hook
// (CJS) can require it without a synchronous ESM import. This ESM wrapper
// re-exports it as the typed/tested surface; the MCP server and the E2E tests
// import from here, the live hook requires the .cjs. ONE implementation.
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const lib = require("./decision-points-lib.cjs");

/** A tool call passes through untouched. */
export const PASS = lib.PASS;
/** The agent is told a better route exists, and may still proceed. */
export const SUGGEST = lib.SUGGEST;
/** Cross-repo discovery: Membrane owns the answer. */
export const ROUTE = lib.ROUTE;

const { SEARCH_TOOLS } = lib;

const {
  DISCOVERY_TERMS,
  textOf,
  looksExact,
  looksBroad,
  namesForeignRepo,
} = lib;

/**
 * Decide how a tool call should be routed. Delegates to the shared CJS
 * implementation so the live Sentinel hook and this ESM surface run identical
 * logic.
 *
 * @returns {{decision:string, reason:string, suggestion?:string}}
 */
export function routeToolCall(event, { knownRepos = [], currentRepo = "" } = {}) {
  return lib.routeToolCall(event, { knownRepos, currentRepo });
}

/**
 * Record that Membrane could not answer and the agent fell back.
 *
 * Plan 2.6 requires the omission be recorded rather than silently swallowed —
 * an unrecorded miss is indistinguishable from a hit that found nothing.
 */
export function fallbackOmission(query, detail = "") {
  return {
    id: "membrane_miss:discovery",
    reason: `membrane_context returned no usable slice for ${JSON.stringify(String(query).slice(0, 120))}${detail ? `: ${detail}` : ""}`,
    layer: 0,
    kind: "membrane_miss",
    severity: "info",
  };
}
