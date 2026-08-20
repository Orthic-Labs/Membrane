// D44: semantic candidate sidecar (S-24) — candidates only, never authority.
// Off by default and local-only in the first implementation. Embeddings can
// never create/upgrade graph edges or claim verdicts/freshness/confidence.

export const semanticCandidateProvider = Object.freeze({
  id: "orthic.semantic.local",
  version: "1.0.0",
  authority: "candidate-only",
  async probe() { return { state: "disabled" }; },
  async search({ query, generationId, limit, signal }) {
    return {
      generationId,
      candidates: [],
      scoreName: "cosine_similarity",
      omissions: [{ reason: "semantic_provider_disabled" }],
    };
  },
});

export function semanticEnabled() {
  return process.env.BLUEPRINT_SEMANTIC === "1";
}
