import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  assertRegisteredRelationshipKinds,
  isRelationshipKind,
  RELATIONSHIP_KINDS,
} from "../src/graph/relationship-kinds.mjs";
import {
  selectTraversalPolicy,
  traversalPolicyFamilies,
  TRAVERSAL_RELATIONSHIP_EXEMPTIONS,
} from "../src/graph/traversal-policy.mjs";
import { pythonScipProvider } from "../src/providers/compilers/python-scip.mjs";

const PYTHON_FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "compiler-adapters", "python");

test("canonical relationship registry includes compiler type usage and documentation traversal", () => {
  assert.ok(isRelationshipKind("TYPED"));
  assert.ok(isRelationshipKind("DOCS_LINK"));
  assert.equal(isRelationshipKind("TYPES"), false, "legacy producer spelling must not become canonical vocabulary");
});

test("first-party SCIP producer emits only canonical relationship kinds", async () => {
  const collected = await pythonScipProvider.collect({ repoRoot: PYTHON_FIXTURE });
  assert.doesNotThrow(() => assertRegisteredRelationshipKinds(collected.edges, "scip-python fixture"));
  assert.ok(collected.edges.some((edge) => edge.kind === "TYPED"));
  assert.ok(!collected.edges.some((edge) => edge.kind === "TYPES"));
});

test("Recall traversal consumer handles or explicitly exempts every canonical relationship", () => {
  const handled = new Set();
  for (const family of traversalPolicyFamilies()) {
    const policy = selectTraversalPolicy("", family);
    for (const kind of policy.kinds) {
      assert.ok(isRelationshipKind(kind), `${family} consumes unknown relationship ${kind}`);
      handled.add(kind);
    }
  }

  for (const [kind, reason] of Object.entries(TRAVERSAL_RELATIONSHIP_EXEMPTIONS)) {
    assert.ok(isRelationshipKind(kind), `exemption references unknown relationship ${kind}`);
    assert.ok(String(reason).trim().length > 0, `${kind} exemption must state why`);
  }

  const unaccounted = RELATIONSHIP_KINDS.filter(
    (kind) => !handled.has(kind) && !TRAVERSAL_RELATIONSHIP_EXEMPTIONS[kind],
  );
  assert.deepEqual(unaccounted, []);
});

test("relationship parity helper fails closed on an undeclared producer kind", () => {
  assert.throws(
    () => assertRegisteredRelationshipKinds([{ kind: "SILENT_NEW_EDGE" }], "fixture-provider"),
    (error) => error?.code === "relationship_kind_unregistered"
      && error.relationshipKinds?.includes("SILENT_NEW_EDGE"),
  );
});
