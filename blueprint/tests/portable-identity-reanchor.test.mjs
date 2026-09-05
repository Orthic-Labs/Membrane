import assert from "node:assert/strict";
import test from "node:test";

import { attachPortableIdentities, portableIdentityKey, portableSemanticIdentity } from "../src/graph/portable-identity.mjs";
import { anchorFingerprint, reanchorEvidence, reconcileRenameAliases } from "../src/graph/reanchor.mjs";

test("portable identity is exact-only and never falls back to same-name merging", () => {
  const scip = portableSemanticIdentity({ symbol: "scip npm pkg 1.0.0 Foo#bar()." });
  assert.ok(portableIdentityKey(scip));
  assert.equal(portableSemanticIdentity({ name: "bar", qualifiedName: "Foo.bar", path: "a.ts" }), null);
  const domain = portableSemanticIdentity({ domainIdentity: { kind: "route", service: "api", method: "GET", path: "/users/:id" } });
  assert.ok(portableIdentityKey(domain));
  assert.notEqual(scip, domain);
});

test("portable relation identity requires exact portable endpoints", () => {
  const generation = {
    nodes: [
      { id: "a", symbol: "scip npm p 1 A#", path: "a.ts" },
      { id: "b", domainIdentity: { kind: "event_topic", address: "created" }, path: "b.ts" },
      { id: "c", name: "sameNameOnly", path: "c.ts" },
    ],
    edges: [
      { id: "ab", kind: "REFERENCES", source: "a", target: "b" },
      { id: "ac", kind: "REFERENCES", source: "a", target: "c" },
    ],
  };
  const result = attachPortableIdentities(generation);
  assert.equal(result.attached, 3);
  assert.ok(generation.nodes[0].portableId);
  assert.ok(generation.nodes[1].portableId);
  assert.equal(generation.nodes[2].portableId, undefined);
  assert.ok(generation.edges[0].portableId);
  assert.equal(generation.edges[1].portableId, undefined);
});

test("reanchoring follows exact entity then exact fingerprint then unique normalized text", () => {
  const exact = reanchorEvidence({ portableId: "bp:domain:sha256:" + "a".repeat(64), text: "old" }, [
    { id: "x", portableId: "bp:domain:sha256:" + "a".repeat(64), text: "new" },
  ]);
  assert.equal(exact.state, "reanchored");
  assert.equal(exact.tier, "exact_entity");

  const fingerprint = anchorFingerprint("keep this exact text");
  const byFingerprint = reanchorEvidence({ fingerprint }, [{ id: "y", text: "keep this exact text" }]);
  assert.equal(byFingerprint.tier, "exact_fingerprint");

  const byText = reanchorEvidence({ text: "hello    world" }, [{ id: "z", text: "hello world" }]);
  assert.equal(byText.tier, "unique_normalized_text");
});

test("reanchoring refuses ambiguity and staleness instead of choosing a fuzzy winner", () => {
  // Both candidates normalize to the same text, but neither is an exact-byte
  // fingerprint match for the prior anchor. The normalized-text tier must
  // therefore stop ambiguous rather than choosing either candidate.
  const ambiguous = reanchorEvidence({ text: "same  text" }, [{ id: "a", text: "same text" }, { id: "b", text: "same   text" }]);
  assert.equal(ambiguous.state, "ambiguous");
  assert.deepEqual(ambiguous.candidates, ["a", "b"]);
  const stale = reanchorEvidence({ text: "gone" }, [{ id: "x", text: "different" }]);
  assert.equal(stale.state, "stale");
  assert.equal(stale.reason, "no_exact_reanchor");
});

test("rename reconciliation emits only exact aliases and preserves unresolved rows", () => {
  const portableId = "bp:scip:sha256:" + "b".repeat(64);
  const result = reconcileRenameAliases({
    oldPath: "src/old.ts",
    newPath: "src/new.ts",
    before: [
      { id: "old:stable", path: "src/old.ts", portableId },
      { id: "old:ambiguous", path: "src/old.ts", text: "dup" },
    ],
    after: [
      { id: "new:stable", path: "src/new.ts", portableId },
      { id: "new:a", path: "src/new.ts", text: "dup" },
      { id: "new:b", path: "src/new.ts", text: "dup" },
    ],
  });
  assert.deepEqual(result.aliases, [{ from: "old:stable", to: "new:stable", tier: "exact_entity", oldPath: "src/old.ts", newPath: "src/new.ts" }]);
  assert.equal(result.unresolved[0].state, "ambiguous");
});
