// Generation identity — body hash (generationId) vs manifest metadata (manifestDigest).
//
// generationId binds the full node/edge set plus source tree hash. It must be
// sealed only after augmentation so lexical and AST layers share one identity.
//
// manifestDigest binds publish metadata downstream consumers pin without loading
// the graph body: provider, composition, counts, schema version, sourceObservation.
//
// W2/CX-R009–R012 identity contract (SEAM-CONTRACT §4.1/§4.2):
//   - The manifestDigest surface is the ONLY place build identity is computed.
//     Hub protocol/lease/endpoint/instance/fence state NEVER enters build identity
//     and never forces a graph rebuild. `computeManifestDigest` binds an explicit
//     whitelist, so any hub field added to the manifest is ignored by the digest,
//     and `finalizeGenerationIdentity` rejects it loudly at seal time.
//   - No duplicate "shadow" manifest keys: a component may be expressed once,
//     under its canonical key. `detectShadowManifestKeys` names any second key
//     that would redundantly re-express the same identity component.

import { createHash } from "node:crypto";
import { createXXHash128 } from "hash-wasm";

const xxhasher = await createXXHash128();

function xxh128(value) {
  xxhasher.init();
  xxhasher.update(value);
  return xxhasher.digest("hex");
}

export function contentDigest(bytes) {
  return `xxh128:${xxh128(bytes)}`;
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map((item) => stableStringify(item)).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}

export function computeGenerationId(nodes, edges, sourceHash) {
  return `xxh128:${xxh128(JSON.stringify({ nodes, edges, sourceHash }))}`;
}

// Hub lifecycle fields that must NEVER enter build identity or force a rebuild.
// These are the exact §4.2 channel fields: the Hub owns protocol negotiation,
// lease issuance, endpoint registration, instance identity, and fencing.
export const HUB_LIFECYCLE_IDENTITY_KEYS = Object.freeze([
  "hub",
  "lease",
  "endpoint",
  "instance",
  "fence",
  "protocol",
]);

// Keys that, if present at the manifest top level, would duplicate a canonical
// identity component expressed elsewhere. Presence is a contract violation, not
// merely an ignored extra: two keys expressing one component is how a graph
// rebuild (or a skip) can be forced by the wrong surface.
export const SHADOW_IDENTITY_ALIASES = Object.freeze({
  sourceHash: "repo.sourceHash",
  sourceTree: "repo.sourceHash",
  sourceTreeHash: "repo.sourceHash",
  treeHash: "repo.sourceHash",
  fileManifest: "repo.fileCount + counts",
  root: "repo.rootName",
  rootName: "repo.rootName",
  repoRoot: "envelope.repoRoot",
  generation: "generationId",
  snapshotId: "generationId",
  resolverUniverse: "providerComposition",
  indexPlan: "providerComposition",
});

/** List hub lifecycle keys present on the manifest (build-identity leaks). */
export function detectHubIdentityFields(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) return [];
  return HUB_LIFECYCLE_IDENTITY_KEYS.filter((key) => Object.hasOwn(manifest, key));
}

/** List shadow keys present on the manifest, mapped to their canonical component. */
export function detectShadowManifestKeys(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) return [];
  return Object.keys(SHADOW_IDENTITY_ALIASES)
    .filter((key) => Object.hasOwn(manifest, key))
    .map((key) => ({ key, canonical: SHADOW_IDENTITY_ALIASES[key] }));
}

/**
 * Reject a manifest that leaks hub lifecycle fields or duplicate shadow keys
 * into build identity. Called at seal time so a leak fails loudly instead of
 * silently entering (or being ignored by) the digest.
 */
export function assertBuildIdentityClean(manifest) {
  const hubFields = detectHubIdentityFields(manifest);
  const shadowKeys = detectShadowManifestKeys(manifest);
  if (hubFields.length || shadowKeys.length) {
    const error = new Error("build_identity_violation");
    error.code = "build_identity_violation";
    error.details = { hubFields, shadowKeys };
    throw error;
  }
  return manifest;
}

/**
 * The explicit identity surface bound by `manifestDigest`. This is the complete
 * BuildSnapshotV1 metadata identity:
 *   - schema            → schemaVersion
 *   - registry/providers→ provider + providerComposition
 *   - root / source/tree→ repo (rootName, sourceHash, fileCount)
 *   - file manifest     → repo.fileCount + counts
 *   - source observation→ sourceObservation (commit + dirty classification)
 * The body identity (generationId) is bound separately by `computeGenerationId`
 * over nodes + edges + sourceHash; ignore/config enters through the scanned-file
 * sourceHash, which already excludes configured ignored prefixes.
 */
export function buildIdentitySurface(manifest, sourceObservation = null) {
  return {
    schemaVersion: manifest?.schemaVersion ?? 1,
    provider: manifest?.provider ?? null,
    providerComposition: manifest?.providerComposition ?? null,
    counts: manifest?.counts ?? null,
    repo: manifest?.repo ?? null,
    sourceObservation: sourceObservation ?? manifest?.sourceObservation ?? null,
  };
}

export function computeManifestDigest(manifest, sourceObservation = null) {
  const surface = buildIdentitySurface(manifest, sourceObservation);
  return `sha256:${createHash("sha256").update(stableStringify(surface)).digest("hex")}`;
}

/** Re-seal generationId + manifestDigest on the in-memory generation object. */
export function finalizeGenerationIdentity(generation) {
  const manifest = generation?.manifest;
  if (!manifest) return generation;
  assertBuildIdentityClean(manifest);
  const sourceHash = manifest.repo?.sourceHash ?? null;
  const id = computeGenerationId(generation.nodes ?? [], generation.edges ?? [], sourceHash);
  manifest.generationId = id;
  manifest.generatedAt = `gen:${id.replace(/^xxh128:/, "").slice(0, 16)}`;
  manifest.manifestDigest = computeManifestDigest(manifest, generation.sourceObservation);
  if (generation.docTruth && generation.provider) {
    generation.docTruth.provider = typeof generation.provider === "string"
      ? generation.provider
      : generation.provider.id;
  }
  return generation;
}

export function resealGenerationIdentityDelta(envelope, rootDigest, appliedClock) {
  const manifest = envelope?.manifest;
  if (!manifest) throw new Error("generation envelope is missing manifest");
  const previousGenerationId = manifest.generationId ?? null;
  const generationId = `xxh128:${xxh128(stableStringify({ previousGenerationId, rootMerkleDigest: rootDigest, appliedClock }))}`;
  manifest.generationId = generationId;
  manifest.generatedAt = `gen:${generationId.replace(/^xxh128:/, "").slice(0, 16)}`;
  manifest.manifestDigest = computeManifestDigest(manifest, envelope.sourceObservation);
  return envelope;
}
