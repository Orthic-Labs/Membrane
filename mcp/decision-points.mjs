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

/** A tool call passes through untouched. */
export const PASS = "pass";
/** The agent is told a better route exists, and may still proceed. */
export const SUGGEST = "suggest";
/** Cross-repo discovery: Membrane owns the answer. */
export const ROUTE = "route";

const SEARCH_TOOLS = new Set(["Grep", "Glob", "Bash", "Search"]);

// A path fragment specific enough to be a real target rather than a sweep.
const EXACT_PATH = /(^|[\s"'])[\w./-]+\.[a-z0-9]{1,5}(:\d+)?([\s"']|$)/i;

// Shell greps that name a concrete file or directory target.
//
// `-r <word>` is NOT evidence of scoping: in `grep -r handler .` the word after
// -r is the PATTERN and the search target is the whole tree. Scope evidence is
// an --include/--exclude filter or an actual path (something containing a `/`
// or a file extension).
const SCOPED_SEARCH = /(--include|--exclude|\s[\w.-]*\/[\w./-]*(\s|$))/;

// Symbol-shaped queries: an agent looking for one named thing.
const SYMBOL_QUERY = /^[\w$]{3,}$/;

/**
 * Terms that imply "where does X live across the workspace" rather than
 * "show me this file". These are the queries a lexical sweep answers badly and
 * a generation-bound graph answers well.
 */
const DISCOVERY_TERMS = [
  "architecture",
  "where is",
  "how does",
  "entry point",
  "call site",
  "who calls",
  "implementation of",
  "defined in",
  "flow",
  "pipeline",
];

function textOf(event) {
  const input = event?.tool_input || event?.toolInput || {};
  return String(
    input.pattern || input.query || input.command || input.prompt || event?.command || "",
  );
}

function looksExact(text) {
  // SYMBOL_QUERY is anchored, so it only matches when the WHOLE query is one
  // symbol (a Grep pattern like "buildNeighborhood"). A shell command such as
  // `grep -r handler .` is not a symbol lookup even though it contains one —
  // testing the full command against an anchored pattern already excludes it,
  // but the ordering matters: check path/scope evidence first so a command is
  // judged on its scoping, not on any word inside it.
  return EXACT_PATH.test(text) || SCOPED_SEARCH.test(text) || SYMBOL_QUERY.test(text.trim());
}

function looksBroad(text) {
  const lower = text.toLowerCase();
  if (DISCOVERY_TERMS.some((term) => lower.includes(term))) return true;
  // An unscoped recursive grep over the tree is a sweep by construction.
  if (/\bgrep\b/.test(lower) && /-r\b/.test(lower) && !SCOPED_SEARCH.test(text)) return true;
  if (/\brg\b/.test(lower) && !SCOPED_SEARCH.test(text) && !EXACT_PATH.test(text)) return true;
  return false;
}

/**
 * Does this text name a repository other than the one we are in?
 * @param {string} text
 * @param {string[]} knownRepos repo names/aliases from the catalog
 */
function namesForeignRepo(text, knownRepos, currentRepo) {
  const lower = text.toLowerCase();
  return knownRepos.some((repo) => {
    const name = String(repo || "").toLowerCase();
    if (!name || name === String(currentRepo || "").toLowerCase()) return false;
    return new RegExp(`\\b${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`).test(lower);
  });
}

/**
 * Decide how a tool call should be routed.
 *
 * @returns {{decision:string, reason:string, suggestion?:string}}
 */
export function routeToolCall(event, { knownRepos = [], currentRepo = "" } = {}) {
  const tool = String(event?.tool_name || event?.toolName || event?.tool || "");
  if (!SEARCH_TOOLS.has(tool)) {
    return { decision: PASS, reason: "not_a_discovery_tool" };
  }

  const text = textOf(event);
  if (!text.trim()) return { decision: PASS, reason: "empty_query" };

  if (namesForeignRepo(text, knownRepos, currentRepo)) {
    return {
      decision: ROUTE,
      reason: "cross_repo_discovery",
      suggestion:
        "This names another repository. Use membrane_context(scope=\"workspace\") — " +
        "it resolves the repo by alias and returns a generation-bound slice with provenance, " +
        "rather than grepping a tree that may not be indexed here.",
    };
  }

  // Exactness wins over breadth: a query naming a concrete file is precise even
  // if it also contains a discovery word.
  if (looksExact(text)) {
    return { decision: PASS, reason: "exact_target" };
  }

  if (looksBroad(text)) {
    return {
      decision: SUGGEST,
      reason: "broad_discovery",
      suggestion:
        "Broad discovery: membrane_context answers this from the current Cortex " +
        "generation with file/span provenance, usually in fewer tokens than a tree-wide sweep. " +
        "Proceeding with the search is still allowed.",
    };
  }

  return { decision: PASS, reason: "scoped_enough" };
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
