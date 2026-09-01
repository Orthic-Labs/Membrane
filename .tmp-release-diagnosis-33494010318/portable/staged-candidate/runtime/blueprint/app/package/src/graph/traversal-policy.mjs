const POLICY_DEFINITIONS = Object.freeze({
  "dependency.forward": { direction: "out", kinds: ["IMPORTS", "CALLS", "CONFIGURES"], maxHops: 3 },
  "impact.reverse": { direction: "in", kinds: ["IMPORTS", "CALLS", "TESTS", "CONFIGURES", "REFERENCES"], maxHops: 3 },
  "callgraph.forward": { direction: "out", kinds: ["CALLS"], maxHops: 4 },
  "test.coverage": { direction: "both", kinds: ["TESTS", "REFERENCES", "IMPORTS"], maxHops: 3 },
  "config.consumers": { direction: "out", kinds: ["CONFIGURES", "REFERENCES"], maxHops: 3 },
  "architecture.boundary": { direction: "both", kinds: ["IMPORTS", "CALLS", "CONTAINS", "DEFINES", "REFERENCES", "DOCS_LINK"], maxHops: 2 },
  "explore.both": { direction: "both", kinds: ["IMPORTS", "CALLS", "TESTS", "CONFIGURES", "CONTAINS", "DEFINES", "REFERENCES", "DOCS_LINK"], maxHops: 2 },
});

const DEFAULT_LIMITS = Object.freeze({
  maxSeeds: 8,
  maxPaths: 40,
  maxNodes: 160,
  maxEdges: 320,
  evidenceRequired: true,
});

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
