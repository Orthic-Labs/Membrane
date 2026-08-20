'use strict';
// Single source of truth for the decision-point routing logic (plan 2.6).
//
// This file is CommonJS so the Forge PreToolUse hook (CJS) can require it
// directly without a synchronous ESM import (which Node forbids). The ESM
// wrapper `decision-points.mjs` re-exports the same values for the typed
// test surface and MCP server. Both consumers share ONE implementation so a
// test against either exercises the real routing the live hook runs.

/** A tool call passes through untouched. */
const PASS = 'pass';
/** The agent is told a better route exists, and may still proceed. */
const SUGGEST = 'suggest';
/** Cross-repo discovery: Membrane owns the answer. */
const ROUTE = 'route';

const SEARCH_TOOLS = new Set(['Grep', 'Glob', 'Bash', 'Search']);

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
  'architecture',
  'where is',
  'how does',
  'entry point',
  'call site',
  'who calls',
  'implementation of',
  'defined in',
  'flow',
  'pipeline',
];

function textOf(event) {
  const input = (event && (event.tool_input || event.toolInput)) || {};
  return String(
    input.pattern || input.query || input.command || input.prompt || (event && event.command) || '',
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
 * @param {string} currentRepo
 */
function namesForeignRepo(text, knownRepos, currentRepo) {
  const lower = text.toLowerCase();
  return knownRepos.some((repo) => {
    const name = String(repo || '').toLowerCase();
    if (!name || name === String(currentRepo || '').toLowerCase()) return false;
    return new RegExp(`\\b${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\b`).test(lower);
  });
}

/**
 * Decide how a tool call should be routed.
 *
 * @returns {{decision:string, reason:string, suggestion?:string}}
 */
function routeToolCall(event, options) {
  const opts = options || {};
  const knownRepos = opts.knownRepos || [];
  const currentRepo = opts.currentRepo || '';
  const tool = String((event && (event.tool_name || event.toolName || event.tool)) || '');
  if (!SEARCH_TOOLS.has(tool)) {
    return { decision: PASS, reason: 'not_a_discovery_tool' };
  }

  const text = textOf(event);
  if (!text.trim()) return { decision: PASS, reason: 'empty_query' };

  if (namesForeignRepo(text, knownRepos, currentRepo)) {
    return {
      decision: ROUTE,
      reason: 'cross_repo_discovery',
      suggestion:
        'This names another repository. Use membrane_context(scope="workspace") — ' +
        'it resolves the repo by alias and returns a generation-bound slice with provenance, ' +
        'rather than grepping a tree that may not be indexed here.',
    };
  }

  // Exactness wins over breadth: a query naming a concrete file is precise even
  // if it also contains a discovery word.
  if (looksExact(text)) {
    return { decision: PASS, reason: 'exact_target' };
  }

  if (looksBroad(text)) {
    return {
      decision: SUGGEST,
      reason: 'broad_discovery',
      suggestion:
        'Broad discovery: membrane_context answers this from the current Blueprint ' +
        'generation with file/span provenance, usually in fewer tokens than a tree-wide sweep. ' +
        'Proceeding with the search is still allowed.',
    };
  }

  return { decision: PASS, reason: 'scoped_enough' };
}

module.exports = {
  PASS,
  SUGGEST,
  ROUTE,
  SEARCH_TOOLS,
  DISCOVERY_TERMS,
  routeToolCall,
  textOf,
  looksExact,
  looksBroad,
  namesForeignRepo,
};
