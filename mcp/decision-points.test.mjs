// Contract tests for decision-point routing (plan 2.6).
//
// The rule under test: precise operations must never be interrupted, broad
// discovery should be offered a better route, and cross-repo discovery must be
// routed through Membrane. Nothing here may ever block.

import assert from "node:assert/strict";
import test from "node:test";

import { PASS, ROUTE, SUGGEST, fallbackOmission, routeToolCall } from "./decision-points.mjs";

const REPOS = ["cortex", "membrane", "forge", "adapt", "heardright"];
const opts = { knownRepos: REPOS, currentRepo: "membrane" };

function grep(pattern) {
  return { tool_name: "Grep", tool_input: { pattern } };
}
function bash(command) {
  return { tool_name: "Bash", tool_input: { command } };
}

test("non-discovery tools pass untouched", () => {
  assert.equal(routeToolCall({ tool_name: "Edit" }, opts).decision, PASS);
  assert.equal(routeToolCall({ tool_name: "Write" }, opts).decision, PASS);
});

test("an exact file target passes", () => {
  // Plan 2.6: exact-file reads/greps pass. Interrupting these would be pure cost.
  assert.equal(routeToolCall(grep("freshness.rs"), opts).decision, PASS);
  assert.equal(routeToolCall(bash("grep -n classify engine/src/freshness.rs"), opts).decision, PASS);
});

test("a symbol-shaped query passes", () => {
  assert.equal(routeToolCall(grep("buildNeighborhood"), opts).decision, PASS);
});

test("a scoped search passes", () => {
  assert.equal(routeToolCall(bash("grep -r foo engine/federation/"), opts).decision, PASS);
  assert.equal(routeToolCall(bash("grep -r foo . --include=*.rs"), opts).decision, PASS);
});

test("broad discovery gets a suggestion but is never blocked", () => {
  const out = routeToolCall(grep("where is the packet budget applied"), opts);
  assert.equal(out.decision, SUGGEST);
  assert.match(out.suggestion, /membrane_context/);
  assert.match(out.suggestion, /still allowed/);
  assert.notEqual(out.decision, "block");
});

test("an unscoped tree-wide sweep is treated as broad", () => {
  assert.equal(routeToolCall(bash("grep -r handler ."), opts).decision, SUGGEST);
});

test("cross-repo discovery routes through Membrane", () => {
  const out = routeToolCall(grep("how does heardright handle uploads"), opts);
  assert.equal(out.decision, ROUTE);
  assert.match(out.suggestion, /scope="workspace"/);
});

test("naming the CURRENT repo is not cross-repo", () => {
  // Being in membrane and saying "membrane" is not a federation event.
  const out = routeToolCall(grep("membrane.rs"), opts);
  assert.equal(out.decision, PASS);
});

test("exactness beats a discovery word", () => {
  // "flow" is a discovery term, but the query names a concrete file.
  assert.equal(routeToolCall(grep("dataflow.mjs"), opts).decision, PASS);
});

test("an empty query passes rather than erroring", () => {
  assert.equal(routeToolCall({ tool_name: "Grep", tool_input: {} }, opts).decision, PASS);
});

test("a Membrane miss produces a recordable omission", () => {
  // Plan 2.6: fallback is allowed, but the miss must be recorded — an
  // unrecorded miss looks identical to a successful empty answer.
  const omission = fallbackOmission("where is the retry budget", "no candidates");
  assert.equal(omission.kind, "membrane_miss");
  assert.match(omission.reason, /retry budget/);
  assert.equal(omission.layer, 0);
});
