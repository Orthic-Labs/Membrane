// Canonical relationship vocabulary. Confidence is derived from the
// resolution path, never hardcoded per edge kind.
//
// INV-021: this registry is executable, not documentation. First-party
// producers may emit only registered kinds, and graph consumers must either
// handle a registered kind or declare an explicit, reason-coded exemption.

export const RELATIONSHIP_REGISTRY = Object.freeze({
  INHERITS: Object.freeze({ category: "type" }),
  IMPLEMENTS: Object.freeze({ category: "type" }),
  OVERRIDES: Object.freeze({ category: "type" }),
  MIXES_IN: Object.freeze({ category: "type" }),
  TYPED: Object.freeze({ category: "type" }),
  USES: Object.freeze({ category: "dependency" }),
  TESTS: Object.freeze({ category: "test" }),
  COVERS: Object.freeze({ category: "test" }),
  GENERATES: Object.freeze({ category: "generation" }),
  IMPORTS: Object.freeze({ category: "dependency" }),
  CALLS: Object.freeze({ category: "execution" }),
  CONTAINS: Object.freeze({ category: "structure" }),
  REFERENCES: Object.freeze({ category: "reference" }),
  DEFINES: Object.freeze({ category: "structure" }),
  CONFIGURES: Object.freeze({ category: "configuration" }),
  READS: Object.freeze({ category: "data" }),
  WRITES: Object.freeze({ category: "data" }),
  PRODUCES: Object.freeze({ category: "flow" }),
  CONSUMES: Object.freeze({ category: "flow" }),
  DEPLOYS: Object.freeze({ category: "deployment" }),
  HANDLES: Object.freeze({ category: "execution" }),
  ROUTES_TO: Object.freeze({ category: "routing" }),
  AUTHORED_BY: Object.freeze({ category: "history" }),
  READ_DURING: Object.freeze({ category: "history" }),
  CHANGED_BY: Object.freeze({ category: "history" }),
  // Architecture/doc-truth traversal consumes this relation even when a
  // particular generation contains no such edge. Keeping it registered makes
  // consumer vocabulary subject to the same parity gate as producer output.
  DOCS_LINK: Object.freeze({ category: "documentation" }),
});

export const RELATIONSHIP_KINDS = Object.freeze(Object.keys(RELATIONSHIP_REGISTRY));

export function isRelationshipKind(kind) {
  return Object.hasOwn(RELATIONSHIP_REGISTRY, kind);
}

export function assertRegisteredRelationshipKinds(edges, context = "graph") {
  const unknown = [...new Set((edges ?? [])
    .map((edge) => edge?.kind)
    .filter((kind) => typeof kind === "string" && !isRelationshipKind(kind)))]
    .sort();
  if (!unknown.length) return;
  const error = new Error(`${context} emitted unregistered relationship kind(s): ${unknown.join(", ")}`);
  error.code = "relationship_kind_unregistered";
  error.relationshipKinds = unknown;
  throw error;
}

// Confidence derived from resolution path per D27 step 1.
export function confidenceFromResolutionPath(path) {
  if (path === "compiler" || path === "scip") return "EXACT_RESOLUTION";
  if (path === "same-file") return "SAME_FILE_LEXICAL";
  if (path === "cross-file-heuristic") return "CROSS_FILE_HEURISTIC";
  return "UNRESOLVED";
}
