import { readFileSync } from "node:fs";
import { join, resolve as resolvePath, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { assertRegisteredRelationshipKinds } from "../graph/relationship-kinds.mjs";
import { FACT_PROVENANCE, withFactProvenance } from "../graph/provenance.mjs";
import { evaluateEvidence } from "../graph/evidence-authority.mjs";
import { ProviderRegistry, runProvider, runProviderSync } from "./index.mjs";
import { pythonScipProvider } from "./compilers/python-scip.mjs";

export const FIRST_PARTY_SEMANTIC_PROVIDERS = Object.freeze([pythonScipProvider]);

/**
 * Licences a provider may declare to be admitted into the registry. First
 * party providers ship under the repository's own licence; anything else has
 * to be added here deliberately.
 */
export const ALLOWED_PROVIDER_LICENSES = Object.freeze(["SEE LICENSE IN LICENSE"]);

/** Root of the blueprint package; provider manifest `entry` paths are relative to it. */
const PACKAGE_ROOT = fileURLToPath(new URL("../../", import.meta.url));

/** Directory holding one committed manifest per first-party provider. */
export const PROVIDER_MANIFEST_DIR = join(PACKAGE_ROOT, "src", "providers", "manifests");

/**
 * Providers that MUST have a committed manifest. A first-party provider with
 * no manifest is refused rather than silently registered unvalidated.
 */
const MANIFEST_REQUIRED_PROVIDER_IDS = new Set(FIRST_PARTY_SEMANTIC_PROVIDERS.map((provider) => provider.id));

function orchestratorError(code, message, details = undefined) {
  const error = new Error(message);
  error.code = code;
  if (details !== undefined) error.details = details;
  return error;
}

/**
 * Load the committed manifest for a provider (BPT-010).
 *
 * The manifest is independent of the provider object: it is a file on disk
 * declaring id, version, licence, entry and the sha256 of the provider's own
 * module bytes. Registration compares the provider's declared identity against
 * it and hashes the artifact at `entry`, so identity drift, an unlisted
 * licence, or a tampered provider module all fail closed at build time.
 * Previously the manifest was synthesized from the provider being validated,
 * so every check compared a value to itself and could not fail.
 *
 * Returns null for a provider with no committed manifest, unless it is a
 * first-party provider, in which case a missing manifest is an error.
 */
export function loadFirstPartyProviderManifest(provider) {
  const path = join(PROVIDER_MANIFEST_DIR, `${provider.id}.json`);
  let raw;
  try {
    raw = readFileSync(path, "utf8");
  } catch (error) {
    if (MANIFEST_REQUIRED_PROVIDER_IDS.has(provider.id)) {
      throw orchestratorError("semantic_provider_manifest_missing", `first-party provider ${provider.id} has no committed manifest at ${path}`, { path, cause: String(error?.message ?? error) });
    }
    return null;
  }
  try {
    return Object.freeze(JSON.parse(raw));
  } catch (error) {
    throw orchestratorError("semantic_provider_manifest_unreadable", `provider manifest ${path} is not valid JSON`, { path, cause: String(error?.message ?? error) });
  }
}

/**
 * Read the provider module bytes the manifest attests. The manifest `entry` is
 * package-relative and must stay inside the package; anything else is refused
 * before the file is opened.
 */
export function readProviderArtifactBytes(manifest) {
  const entry = String(manifest?.entry ?? "").replaceAll("\\", "/");
  const absolute = resolvePath(PACKAGE_ROOT, entry);
  if (!absolute.startsWith(PACKAGE_ROOT.endsWith(sep) ? PACKAGE_ROOT : `${PACKAGE_ROOT}${sep}`)) {
    throw orchestratorError("semantic_provider_artifact_escapes_package", `provider entry ${entry} escapes the package root`);
  }
  try {
    return readFileSync(absolute);
  } catch (error) {
    throw orchestratorError("semantic_provider_artifact_missing", `provider artifact ${entry} could not be read`, { entry, cause: String(error?.message ?? error) });
  }
}

function typedDisposition(provider, disposition, detail = {}) {
  return Object.freeze({
    provider: provider.id,
    version: provider.version,
    disposition,
    ...detail,
  });
}

function validateSemanticOutput(provider, output) {
  if (!output || typeof output !== "object" || Array.isArray(output)) {
    const error = new Error(`semantic provider ${provider.id} returned a non-object result`);
    error.code = "semantic_provider_output_invalid";
    throw error;
  }
  for (const field of ["nodes", "edges", "reports"]) {
    if (output[field] !== undefined && !Array.isArray(output[field])) {
      const error = new Error(`semantic provider ${provider.id} returned non-array ${field}`);
      error.code = "semantic_provider_output_invalid";
      throw error;
    }
  }
  assertRegisteredRelationshipKinds(output.edges ?? [], provider.id);
  return {
    ...output,
    nodes: output.nodes ?? [],
    edges: output.edges ?? [],
    reports: output.reports ?? [],
  };
}

function dispositionForProbe(probe) {
  const state = String(probe?.state ?? "unknown").toLowerCase();
  if (["ok", "available", "ready"].includes(state)) return "available";
  if (["partial", "degraded"].includes(state)) return "partial";
  if (["disabled", "not_configured"].includes(state)) return "disabled";
  if (["unavailable", "unsupported", "missing"].includes(state)) return "unsupported";
  return "unknown";
}

function unavailableSemanticOutput(provider, probe) {
  return {
    nodes: [],
    edges: [],
    reports: [{
      kind: probe?.state ?? "unavailable",
      code: probe?.code ?? null,
      reason: probe?.reason ?? null,
      degradesTo: probe?.degradesTo ?? null,
      provider: provider.id,
      precisionTier: probe?.precisionTier ?? null,
    }],
    index: probe ?? { state: "unavailable", provider: provider.id },
  };
}

export function createSemanticProviderRegistry({
  providers = FIRST_PARTY_SEMANTIC_PROVIDERS,
  allowedLicenses = ALLOWED_PROVIDER_LICENSES,
  manifestFor = loadFirstPartyProviderManifest,
} = {}) {
  const registry = new ProviderRegistry({ allowedLicenses });
  for (const provider of providers) {
    if (provider.kind !== "compiler") {
      throw orchestratorError("semantic_provider_kind_invalid", `semantic provider ${provider.id} must be kind=compiler`);
    }
    // Registering WITH a committed manifest AND the provider's real module
    // bytes is what puts identity, licence and checksum validation on the
    // production path. Registering without them silently skips all three.
    const manifest = manifestFor(provider);
    registry.register(provider, manifest
      ? { manifest, artifactBytes: readProviderArtifactBytes(manifest) }
      : {});
  }
  return registry;
}

/**
 * Providers admitted for execution, resolved THROUGH the registry.
 *
 * Both lanes iterate this rather than the caller's array, so a provider that
 * fails registration (identity drift, unlisted licence, tampered module bytes)
 * never runs — registration is load-bearing, not advisory.
 */
function admittedProviders(providers) {
  const registry = createSemanticProviderRegistry({ providers });
  // Caller order is preserved; `registry.list()` sorts by id.
  return providers.map((provider) => registry.get(provider.id).provider);
}

/**
 * Synchronous in-process lane used by the existing synchronous build path.
 * It centralizes probe/output/relationship validation without introducing a
 * second graph builder. Providers that need process execution use the async
 * bounded lane below instead.
 */
export function collectSemanticEvidenceSync(context = {}, {
  providers = FIRST_PARTY_SEMANTIC_PROVIDERS,
  signal = null,
  allowProcess = false,
} = {}) {
  const results = [];
  for (const provider of admittedProviders(providers)) {
    let probe;
    try {
      probe = runProviderSync(provider, context, { signal, allowProcess, operation: "probe" });
    } catch (error) {
      results.push({
        provider,
        probe: null,
        output: { nodes: [], edges: [], reports: [] },
        disposition: typedDisposition(provider, "failed", { code: error?.code ?? "provider_probe_failed", reason: String(error?.message ?? error) }),
      });
      continue;
    }
    const probeDisposition = dispositionForProbe(probe);
    if (["unsupported", "disabled"].includes(probeDisposition)) {
      results.push({
        provider,
        probe,
        output: unavailableSemanticOutput(provider, probe),
        disposition: typedDisposition(provider, probeDisposition, { code: probe?.code ?? null, reason: probe?.reason ?? null }),
      });
      continue;
    }
    try {
      const output = validateSemanticOutput(provider, runProviderSync(provider, context, { signal, allowProcess, operation: "collect" }));
      results.push({
        provider,
        probe,
        output,
        disposition: typedDisposition(provider, probeDisposition === "partial" ? "partial" : "indexed", {
          nodes: output.nodes.length,
          edges: output.edges.length,
          reports: output.reports.length,
        }),
      });
    } catch (error) {
      results.push({
        provider,
        probe,
        output: { nodes: [], edges: [], reports: [] },
        disposition: typedDisposition(provider, "failed", { code: error?.code ?? "provider_collect_failed", reason: String(error?.message ?? error) }),
      });
    }
  }
  return Object.freeze({ schemaVersion: 1, results: Object.freeze(results) });
}

/** Bounded/cancellable lane for semantic providers that can run asynchronously. */
export async function collectSemanticEvidence(context = {}, {
  providers = FIRST_PARTY_SEMANTIC_PROVIDERS,
  timeoutMs = 5000,
  signal = null,
  allowProcess = false,
} = {}) {
  const results = [];
  for (const provider of admittedProviders(providers)) {
    try {
      const probe = await runProvider(provider, context, { signal, timeoutMs, allowProcess, operation: "probe" });
      const probeDisposition = dispositionForProbe(probe);
      if (["unsupported", "disabled"].includes(probeDisposition)) {
        results.push({ provider, probe, output: unavailableSemanticOutput(provider, probe), disposition: typedDisposition(provider, probeDisposition, { code: probe?.code ?? null, reason: probe?.reason ?? null }) });
        continue;
      }
      const output = validateSemanticOutput(provider, await runProvider(provider, context, { signal, timeoutMs, allowProcess, operation: "collect" }));
      results.push({ provider, probe, output, disposition: typedDisposition(provider, probeDisposition === "partial" ? "partial" : "indexed", { nodes: output.nodes.length, edges: output.edges.length, reports: output.reports.length }) });
    } catch (error) {
      const disposition = error?.code === "provider_cancelled"
        ? "cancelled"
        : error?.code === "provider_timeout" ? "timed_out" : "failed";
      results.push({ provider, probe: null, output: { nodes: [], edges: [], reports: [] }, disposition: typedDisposition(provider, disposition, { code: error?.code ?? "provider_failed", reason: String(error?.message ?? error) }) });
    }
  }
  return Object.freeze({ schemaVersion: 1, results: Object.freeze(results) });
}

function liveVerificationCandidate(canonical, verification, sourceStateId) {
  const target = verification?.targetId ?? verification?.target ?? verification?.entityId ?? null;
  return withFactProvenance({
    id: `live-verification:${verification?.provider ?? "lsp"}:${canonical?.id ?? target ?? "unknown"}`,
    kind: canonical?.kind ?? verification?.kind ?? null,
    relation: canonical?.relation ?? verification?.relation ?? null,
    source: canonical?.source ?? verification?.source ?? null,
    target,
    targetId: target,
    provider: verification?.provider ?? "lsp",
    sourceStateId: verification?.sourceStateId ?? sourceStateId ?? canonical?.sourceStateId ?? canonical?.generationId ?? null,
    sourceRelation: verification?.sourceRelation ?? "current",
    confidenceTier: verification?.confidenceTier ?? "EXACT_RESOLUTION",
    resolved: typeof target === "string" && target.length > 0,
    evidence: verification?.evidence ?? [],
  }, FACT_PROVENANCE.LIVE_VERIFICATION, null);
}

/**
 * Compare an already-current canonical fact with an on-demand LSP/host result.
 * The verifier can agree or produce a conflict receipt; it can never originate
 * canonical truth or overwrite the canonical target.
 */
export async function crossCheckWithLiveVerifier({
  canonical,
  verifier,
  request = {},
  sourceStateId = null,
  timeoutMs = 1000,
  signal = null,
} = {}) {
  if (!canonical || typeof canonical !== "object") {
    return Object.freeze({ state: "unavailable", reason: "canonical_fact_required", canonical: null, verification: null });
  }
  if (typeof verifier !== "function") {
    return Object.freeze({ state: "unavailable", reason: "live_verifier_unavailable", canonical, verification: null });
  }
  let timer = null;
  let abortListener = null;
  try {
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(() => reject(Object.assign(new Error("live semantic verification timed out"), { code: "live_verification_timeout" })), timeoutMs);
    });
    const cancelled = signal ? new Promise((_, reject) => {
      abortListener = () => reject(Object.assign(new Error("live semantic verification cancelled"), { code: "live_verification_cancelled" }));
      signal.addEventListener("abort", abortListener, { once: true });
    }) : null;
    const work = Promise.resolve().then(() => verifier({ canonical, request, signal }));
    const raw = await Promise.race(cancelled ? [work, timeout, cancelled] : [work, timeout]);
    if (!raw || raw.state === "unavailable" || raw.supported === false) {
      return Object.freeze({ state: "unavailable", reason: raw?.reason ?? "live_verifier_unavailable", canonical, verification: raw ?? null });
    }
    const canonicalTarget = canonical.targetId ?? canonical.target ?? canonical.id ?? null;
    const canonicalForEvaluation = canonical.targetId || canonical.target
      ? canonical
      : {
          ...canonical,
          relation: canonical.relation ?? "DEFINES",
          source: canonical.source ?? canonical.id,
          target: canonicalTarget,
          targetId: canonicalTarget,
          sourceStateId: canonical.sourceStateId ?? sourceStateId,
          sourceRelation: canonical.sourceRelation ?? "current",
          provenance: canonical.provenance ?? FACT_PROVENANCE.RULE_RESOLVED,
          confidenceTier: canonical.confidenceTier ?? "EXACT_RESOLUTION",
          resolved: Boolean(canonicalTarget),
        };
    const verification = liveVerificationCandidate(canonicalForEvaluation, raw, sourceStateId);
    const evaluated = evaluateEvidence({
      targetSourceState: sourceStateId ?? canonicalForEvaluation.sourceStateId ?? canonicalForEvaluation.generationId ?? null,
      candidates: [canonicalForEvaluation, verification],
      requestedRelation: canonicalForEvaluation.relation ?? null,
    });
    if (evaluated.state === "unresolved_frontier" && evaluated.reason === "resolution_conflict") {
      return Object.freeze({ state: "resolution_conflict", reason: evaluated.reason, canonical, verification, evaluation: evaluated });
    }
    return Object.freeze({ state: "agreement", reason: "live_verifier_agrees", canonical, verification, evaluation: evaluated });
  } catch (error) {
    return Object.freeze({ state: "unavailable", reason: error?.code ?? "live_verification_failed", canonical, verification: null, detail: String(error?.message ?? error) });
  } finally {
    if (timer) clearTimeout(timer);
    if (signal && abortListener) signal.removeEventListener("abort", abortListener);
  }
}
