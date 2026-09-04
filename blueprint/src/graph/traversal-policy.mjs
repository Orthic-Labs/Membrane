import { isRelationshipKind, RELATIONSHIP_KINDS } from "./relationship-kinds.mjs";

const POLICY_DEFINITIONS = Object.freeze({
  "dependency.forward": { direction: "out", kinds: ["IMPORTS", "CALLS", "CONFIGURES"], maxHops: 3 },
  "impact.reverse": { direction: "in", kinds: ["IMPORTS", "CALLS", "TESTS", "CONFIGURES", "REFERENCES"], maxHops: 3 },
  "callgraph.forward": { direction: "out", kinds: ["CALLS"], maxHops: 4 },
  "test.coverage": { direction: "both", kinds: ["TESTS", "REFERENCES", "IMPORTS"], maxHops: 3 },
  "config.consumers": { direction: "out", kinds: ["CONFIGURES", "REFERENCES"], maxHops: 3 },
  "architecture.boundary": { direction: "both", kinds: ["IMPORTS", "CALLS", "CONTAINS", "DEFINES", "REFERENCES", "DOCS_LINK"], maxHops: 2 },
  "explore.both": { direction: "both", kinds: ["IMPORTS", "CALLS", "TESTS", "CONFIGURES", "CONTAINS", "DEFINES", "REFERENCES", "DOCS_LINK"], maxHops: 2 },
});

// INV-021 consumer parity. Recall traversal is intentionally not the consumer
// for every graph relationship; specialized architecture/impact/history/type
// projections own some relations. Those omissions must be explicit so a newly
// registered relationship cannot silently vanish from traversal policy.
export const TRAVERSAL_RELATIONSHIP_EXEMPTIONS = Object.freeze({
  INHERITS: "type-hierarchy relation; consumed by type/architecture projections rather than generic Recall traversal",
  IMPLEMENTS: "type-hierarchy relation; consumed by type/architecture projections rather than generic Recall traversal",
  MIXES_IN: "type-hierarchy relation; consumed by type/architecture projections rather than generic Recall traversal",
  TYPED: "type-use relation; consumed by semantic/type projections rather than generic Recall traversal",
  USES: "generic dependency relation retained for compatibility; task-specific consumers use more precise relations",
  COVERS: "test-evidence relation; test recommendation/coverage consumers own traversal semantics",
  GENERATES: "generation/provenance relation; provenance consumers own traversal semantics",
  READS: "data-access relation; domain/data projections own traversal semantics",
  WRITES: "data-access relation; domain/data projections own traversal semantics",
  PRODUCES: "event/dataflow relation; domain/architecture projections own traversal semantics",
  CONSUMES: "event/dataflow relation; domain/architecture projections own traversal semantics",
  DEPLOYS: "deployment relation; architecture/deployment projections own traversal semantics",
  HANDLES: "handler relation; framework projections own traversal semantics",
  ROUTES_TO: "routing relation; architecture/route projections own traversal semantics",
  AUTHORED_BY: "history relation; change-intelligence consumers own traversal semantics",
  READ_DURING: "history/session relation; history consumers own traversal semantics",
  CHANGED_BY: "history relation; change-intelligence consumers own traversal semantics",
});

const DEFAULT_LIMITS = Object.freeze({
  maxSeeds: 8,
  maxPaths: 40,
  maxNodes: 160,
  maxEdges: 320,
  evidenceRequired: true,
});

function validatePolicyVocabulary() {
  const handled = new Set();
  for (const [family, definition] of Object.entries(POLICY_DEFINITIONS)) {
    for (const kind of definition.kinds) {
      if (!isRelationshipKind(kind)) {
        const error = new Error(`traversal policy ${family} references unregistered relationship kind: ${kind}`);
        error.code = "relationship_consumer_unregistered_kind";
        throw error;
      }
      handled.add(kind);
    }
  }
  const unknownExemptions = Object.keys(TRAVERSAL_RELATIONSHIP_EXEMPTIONS).filter((kind) => !isRelationshipKind(kind));
  if (unknownExemptions.length) {
    const error = new Error(`traversal policy exemptions reference unregistered relationship kind(s): ${unknownExemptions.join(", ")}`);
    error.code = "relationship_consumer_unknown_exemption";
    throw error;
  }
  const unaccounted = RELATIONSHIP_KINDS.filter((kind) => !handled.has(kind) && !TRAVERSAL_RELATIONSHIP_EXEMPTIONS[kind]);
  if (unaccounted.length) {
    const error = new Error(`traversal policy has no handling or explicit exemption for relationship kind(s): ${unaccounted.join(", ")}`);
    error.code = "relationship_consumer_parity_gap";
    throw error;
  }
}

validatePolicyVocabulary();

export function selectTraversalPolicy(task, requested = null, limits = {}) {
  const text = String(task ?? "").toLowerCase();
  const family = requested
    ?? (/(impact|affected|caller|consumer|break)/.test(text) ? "impact.reverse"
      : /(test|coverage|spec)/.test(text) ? "test.coverage"
      : /(config|setting|environment)/.test(text) ? "config.consumers"
      : /(call|invoke|execution)/.test(text) ? "callgraph.forward"
      : /(architecture|boundary|component|layer)/.test(text) ? "architecture.boundary"
      : /(depend|import|require)/.test(text) ? "dependency.forward"
      : "explore.both");
  const base = POLICY_DEFINITIONS[family];
  if (!base) {
    const error = new Error(`unknown traversal policy: ${family}`);
    error.code = "traversal_policy_unknown";
    throw error;
  }
  const clamp = (key, fallback) => Math.max(1, Math.min(fallback, Number(limits[key] ?? fallback) || fallback));
  return Object.freeze({
    family,
    direction: base.direction,
    kinds: Object.freeze([...base.kinds]),
    maxHops: clamp("maxHops", base.maxHops),
    maxSeeds: clamp("maxSeeds", DEFAULT_LIMITS.maxSeeds),
    maxPaths: clamp("maxPaths", DEFAULT_LIMITS.maxPaths),
    maxNodes: clamp("maxNodes", DEFAULT_LIMITS.maxNodes),
    maxEdges: clamp("maxEdges", DEFAULT_LIMITS.maxEdges),
    evidenceRequired: limits.evidenceRequired ?? DEFAULT_LIMITS.evidenceRequired,
  });
}

export function traversalPolicyFamilies() {
  return Object.keys(POLICY_DEFINITIONS);
}
