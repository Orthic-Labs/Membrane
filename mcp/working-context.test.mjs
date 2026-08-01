import assert from "node:assert/strict";
import test from "node:test";
import { applyTemporalFact, scratchpad, temporalFact, workingContext } from "./working-context.mjs";

test("L3 working context and scratchpad enforce bounds and authority", () => {
  const context = workingContext({ sessionId: "s", taskId: "t", items: [{ ref: "sha256:a" }], expiresAt: "2026-08-01T00:00:00Z", sourceRefs: ["doc-1"] });
  const pad = scratchpad({ sessionId: "s", taskId: "t", items: [{ note: "bounded" }], expiresAt: "2026-08-01T00:00:00Z" });
  assert.equal(context.authority, "A0");
  assert.equal(pad.searchable, false);
  assert.equal(pad.authority, "A0");
  assert.throws(() => scratchpad({ sessionId: "s", taskId: "t", items: Array(129).fill({}), expiresAt: "2026-08-01T00:00:00Z" }));
});

test("L3 temporal facts preserve multi-valued predicates and close only declared single-valued ones", () => {
  const first = temporalFact({ factId: "f1", subject: "repo", predicate: "owner", object: "a", observedAt: "2026-08-01T00:00:00Z", scopeId: "scope-a", authority: "A1", veracity: "supported" });
  const second = temporalFact({ factId: "f2", subject: "repo", predicate: "owner", object: "b", observedAt: "2026-08-02T00:00:00Z", scopeId: "scope-a", authority: "A1", veracity: "supported" });
  assert.equal(applyTemporalFact([first], second).length, 2);
  const closed = applyTemporalFact([first], second, { singleValuedPredicates: ["owner"] });
  assert.equal(closed[0].veracity, "superseded");
  assert.equal(closed[0].valid_until, second.observed_at);
});
