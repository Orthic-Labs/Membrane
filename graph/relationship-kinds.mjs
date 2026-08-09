// D27: common relationship vocabulary. Confidence is derived from the
// resolution path, never hardcoded per edge kind.

export const RELATIONSHIP_KINDS = Object.freeze([
  "INHERITS",
  "IMPLEMENTS",
  "MIXES_IN",
  "USES",
  "TESTS",
  "COVERS",
  "GENERATES",
  "IMPORTS",
  "CALLS",
  "CONTAINS",
  "REFERENCES",
  "DEFINES",
  "CONFIGURES",
  "READS",
  "WRITES",
  "PRODUCES",
  "CONSUMES",
  "DEPLOYS",
  "HANDLES",
  "ROUTES_TO",
  "AUTHORED_BY",
  "READ_DURING",
  "CHANGED_BY",
]);

export function isRelationshipKind(kind) {
  return RELATIONSHIP_KINDS.includes(kind);
}

// Confidence derived from resolution path per D27 step 1.
export function confidenceFromResolutionPath(path) {
  if (path === "compiler" || path === "scip") return "EXACT_RESOLUTION";
  if (path === "same-file") return "SAME_FILE_LEXICAL";
  if (path === "cross-file-heuristic") return "CROSS_FILE_HEURISTIC";
  return "UNRESOLVED";
}
