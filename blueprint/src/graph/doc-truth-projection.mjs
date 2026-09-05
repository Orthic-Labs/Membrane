// Generation-bound document truth projection.
//
// Claims remain declarations. This module compares those declarations with
// deterministic code evidence already persisted by Blueprint and never turns
// documentation prose into observed code truth.

const DETERMINISTIC_CLASSES = new Set(["EXTRACTED", "DETERMINISTIC_EXTRACTION", "AUTHORITATIVE_SEMANTIC"]);

function publicConfidence(edge) {
  return DETERMINISTIC_CLASSES.has(String(edge?.confidenceClass ?? "")) ? null : edge?.confidence ?? null;
}

function citationFor(edge) {
  const doc = edge?.evidenceDocPath ? {
    kind: "document",
    path: edge.evidenceDocPath,
    line: edge.evidenceDocLine ?? null,
    contentHash: edge.evidenceDocSha1 ?? null,
  } : null;
  const code = edge?.evidenceCodePath ? {
    kind: "code",
    path: edge.evidenceCodePath,
    nodeId: edge.evidenceCodeNodeId ?? null,
    contentHash: edge.evidenceCodeContentHash ?? null,
  } : null;
  return [doc, code].filter(Boolean);
}

function groundingState(edges, freshness) {
  if (freshness && freshness !== "fresh") return "stale";
  const kinds = new Set(edges.map((edge) => edge.kind));
  if (kinds.has("supports") && kinds.has("contradicts")) return "ambiguous";
  if (kinds.has("contradicts")) return "contradicted";
  if (kinds.has("supports")) return "direct";
  if (kinds.has("supersedes")) return "indirect";
  return "unsupported";
}

export const GROUNDING_STATES = Object.freeze(["direct", "indirect", "unsupported", "contradicted", "ambiguous", "stale"]);

export function projectDocumentTruth({ claims = [], supersedes = [], generationId = null, freshness = "unknown" } = {}) {
  const grounded = claims.map((claim) => {
    const edges = [...(claim.edges ?? [])];
    const state = groundingState(edges, freshness);
    const citations = [];
    const seen = new Set();
    for (const edge of edges) {
      for (const citation of citationFor(edge)) {
        const key = JSON.stringify(citation);
        if (!seen.has(key)) { seen.add(key); citations.push(citation); }
      }
    }
    const observed = edges.map((edge) => ({
      kind: edge.kind,
      source: edge.source,
      target: edge.target,
      reason: edge.reason ?? null,
      provenance: edge.confidenceClass ?? null,
      confidence: publicConfidence(edge),
      evidence: citationFor(edge),
    }));
    return {
      claimId: claim.id,
      declared: {
        documentId: claim.documentId ?? null,
        source: claim.source ?? null,
        line: claim.line ?? null,
        status: claim.status ?? "unknown",
        sourceHash: claim.sha1 ?? null,
      },
      grounding: state,
      observed,
      mismatch: ["contradicted", "ambiguous"].includes(state)
        ? { present: true, reason: state === "contradicted" ? "declared_intent_conflicts_with_observed_code" : "conflicting_grounding_evidence" }
        : { present: false, reason: null },
      citations,
      confidence: observed.length && observed.every((row) => row.confidence === null) ? null : observed.map((row) => row.confidence).filter((value) => value !== null),
      invalidation: { generationId, freshness, stale: state === "stale" },
    };
  });
  const counts = Object.fromEntries(GROUNDING_STATES.map((state) => [state, grounded.filter((item) => item.grounding === state).length]));
  return Object.freeze({
    schemaVersion: 1,
    kind: "document-truth-grounding",
    generationId,
    freshness,
    claims: grounded,
    supersedes: [...supersedes],
    counts,
  });
}
