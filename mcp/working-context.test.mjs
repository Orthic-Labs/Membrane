import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyTemporalFact, ScratchpadStore, scratchpad, temporalFact, workingContext, WorkingContextStore } from "./working-context.mjs";

test("L3 working context and scratchpad enforce bounds and authority", () => {
  const context = workingContext({ sessionId: "s", taskId: "t", items: [{ ref: "sha256:a" }], expiresAt: "2026-08-01T00:00:00Z", sourceRefs: ["doc-1"] });
  const pad = scratchpad({ sessionId: "s", taskId: "t", items: [{ note: "bounded" }], expiresAt: "2026-08-01T00:00:00Z" });
  assert.equal(context.authority, "A0");
  assert.equal(pad.searchable, false);
  assert.equal(pad.authority, "none");
  assert.equal(pad.durable, false);
  assert.throws(() => scratchpad({ sessionId: "s", taskId: "t", items: Array(129).fill({}), expiresAt: "2026-08-01T00:00:00Z" }));
  assert.throws(() => scratchpad({ sessionId: "s", taskId: "t", items: [{ chain_of_thought: "never store" }], expiresAt: "2026-08-01T00:00:00Z" }), /private_reasoning/);
});

test("L3 temporal facts preserve multi-valued predicates and close only declared single-valued ones", () => {
  const first = temporalFact({ factId: "f1", subject: "repo", predicate: "owner", object: "a", observedAt: "2026-08-01T00:00:00Z", scopeId: "scope-a", authority: "A1", veracity: "supported", sourceRefs: ["source-a"] });
  const second = temporalFact({ factId: "f2", subject: "repo", predicate: "owner", object: "b", observedAt: "2026-08-02T00:00:00Z", scopeId: "scope-a", authority: "A1", veracity: "supported", sourceRefs: ["source-b"] });
  assert.equal(applyTemporalFact([first], second).length, 2);
  const closed = applyTemporalFact([first], second, { singleValuedPredicates: ["owner"] });
  assert.equal(closed[0].veracity, "superseded");
  assert.equal(closed[0].valid_until, second.observed_at);
  assert.equal(closed[1].supersedes, "f1");
});

test("L3 durable working context survives restart, expires, and injects only exact scope", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-working-context-"));
  const path = join(root, "events.sqlite3");
  try {
    let store = new WorkingContextStore(path);
    const saved = store.saveContext({ sessionId: "session-a", taskId: "task-a", items: [{ ref: "sha256:a" }], expiresAt: "2026-08-03T00:00:00Z", durable: true, authority: "A1", sourceRefs: ["source-a"] });
    store.close();
    store = new WorkingContextStore(path);
    assert.deepEqual(store.activeContexts({ sessionId: "session-a", taskId: "task-a", asOf: "2026-08-02T00:00:00Z" }), [saved]);
    assert.deepEqual(store.activeContexts({ sessionId: "session-b", taskId: "task-a", asOf: "2026-08-02T00:00:00Z" }), []);
    assert.deepEqual(store.activeContexts({ sessionId: "session-a", taskId: "task-a", asOf: "2026-08-04T00:00:00Z" }), []);
    assert.equal(store.closeContext(saved.context_id), true);
    assert.deepEqual(store.activeContexts({ sessionId: "session-a", taskId: "task-a", asOf: "2026-08-02T00:00:00Z" }), []);
    store.close();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("MBR-305: durable context history pages by immutable insertion cursor", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-context-page-"));
  const path = join(root, "events.sqlite3");
  try {
    const store = new WorkingContextStore(path);
    for (const contextId of ["context-a", "context-b", "context-c"]) {
      store.saveContext({ contextId, sessionId: "s", taskId: "t", items: [], expiresAt: "2026-08-03T00:00:00Z", durable: true });
    }
    const first = store.activeContextPage({ sessionId: "s", taskId: "t", asOf: "2026-08-02T00:00:00Z", limit: 2 });
    assert.deepEqual(first.items.map((row) => row.context_id), ["context-a", "context-b"]);
    store.saveContext({ contextId: "context-z", sessionId: "s", taskId: "t", items: [], expiresAt: "2026-08-03T00:00:00Z", durable: true });
    const second = store.activeContextPage({ sessionId: "s", taskId: "t", asOf: "2026-08-02T00:00:00Z", cursor: first.nextCursor, limit: 2 });
    assert.deepEqual(second.items.map((row) => row.context_id), ["context-c", "context-z"]);
    assert.equal(second.nextCursor, null);
    store.close();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("L3 temporal as-of closes only explicit single-valued predicates", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-temporal-fact-"));
  const path = join(root, "events.sqlite3");
  const sourceRefs = ["source-a"];
  try {
    let store = new WorkingContextStore(path);
    store.recordTemporalFact({ factId: "f1", subject: "repo", predicate: "owner", object: "a", observedAt: "2026-08-01T00:00:00Z", scopeId: "scope-a", authority: "A1", veracity: "supported", sourceRefs });
    store.recordTemporalFact({ factId: "f2", subject: "repo", predicate: "owner", object: "b", observedAt: "2026-08-02T00:00:00Z", scopeId: "scope-a", authority: "A1", veracity: "supported", sourceRefs }, { singleValuedPredicates: ["owner"] });
    store.recordTemporalFact({ factId: "f3", subject: "repo", predicate: "tag", object: "one", observedAt: "2026-08-01T00:00:00Z", scopeId: "scope-a" });
    store.recordTemporalFact({ factId: "f4", subject: "repo", predicate: "tag", object: "two", observedAt: "2026-08-02T00:00:00Z", scopeId: "scope-a" });
    store.close();
    store = new WorkingContextStore(path);
    assert.deepEqual(store.temporalFactsAsOf({ scopeId: "scope-a", subject: "repo", predicate: "owner", asOf: "2026-08-01T12:00:00Z" }).map((fact) => fact.fact_id), ["f1"]);
    assert.deepEqual(store.temporalFactsAsOf({ scopeId: "scope-a", subject: "repo", predicate: "owner", asOf: "2026-08-02T12:00:00Z" }).map((fact) => fact.fact_id), ["f2"]);
    assert.deepEqual(store.temporalFactsAsOf({ scopeId: "scope-a", subject: "repo", predicate: "tag", asOf: "2026-08-03T00:00:00Z" }).map((fact) => fact.fact_id), ["f3", "f4"]);
    store.close();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("L3 scratchpad is process-memory only and never reaches durable context store", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-scratchpad-"));
  const path = join(root, "events.sqlite3");
  try {
    const pads = new ScratchpadStore();
    pads.save(path, { sessionId: "s", taskId: "t", items: [{ note: "ephemeral" }], expiresAt: "2026-08-03T00:00:00Z" });
    assert.equal(pads.load(path, { sessionId: "s", taskId: "t", asOf: "2026-08-02T00:00:00Z" }).items[0].note, "ephemeral");
    const store = new WorkingContextStore(path);
    assert.deepEqual(store.activeContexts({ sessionId: "s", taskId: "t", asOf: "2026-08-02T00:00:00Z" }), []);
    assert.equal(pads.clear(path, { sessionId: "s", taskId: "t" }), true);
    assert.equal(pads.load(path, { sessionId: "s", taskId: "t", asOf: "2026-08-02T00:00:00Z" }), null);
    store.close();
  } finally { rmSync(root, { recursive: true, force: true }); }
});
