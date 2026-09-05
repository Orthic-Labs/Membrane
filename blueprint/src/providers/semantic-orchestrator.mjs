import { assertRegisteredRelationshipKinds } from "../graph/relationship-kinds.mjs";
import { FACT_PROVENANCE, withFactProvenance } from "../graph/provenance.mjs";
import { evaluateEvidence } from "../graph/evidence-authority.mjs";
import { ProviderRegistry, runProvider } from "./index.mjs";
import { pythonScipProvider } from "./compilers/python-scip.mjs";

export const FIRST_PARTY_SEMANTIC_PROVIDERS = Object.freeze([pythonScipProvider]);

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

export function createSemanticProviderRegistry({ providers = FIRST_PARTY_SEMANTIC_PROVIDERS } = {}) {
  const registry = new ProviderRegistry();
  for (const provider of providers) {
    if (provider.kind !== "compiler") {
      const error = new Error(`semantic provider ${provider.id} must be kind=compiler`);
      error.code = "semantic_provider_kind_invalid";
      throw error;
    }
    registry.register(provider);
  }
  return registry;
}

/**
 * Synchronous in-process lane used by the existing synchronous build path.
 * It centralizes probe/output/relationship validation without introducing a
 * second graph builder. Providers that need process execution use the async
 * bounded lane below instead.
 */
export function collectSemanticEvidenceSync(context = {}, {
  providers = FIRST_PARTY_SEMANTIC_PROVIDERS,
} = {}) {
  createSemanticProviderRegistry({ providers });
  const results = [];
  for (const provider of providers) {
    let probe;
    try {
      probe = provider.probe(context);
    } catch (error) {
      results.push({
        provider,
        probe: null,
        output: { nodes: [], edges: [], reports: [] },
        disposition: typedDisposition(provider, "failed", { code: "provider_probe_failed", reason: String(error?.message ?? error) }),
      });
      continue;
    }
    const probeDisposition = dispositionForProbe(probe);
    if (["unsupported", "disabled"].includes(probeDisposition)) {
      results.push({
        provider,
        probe,
        output: { nodes: [], edges: [], reports: [] },
        disposition: typedDisposition(provider, probeDisposition, { code: probe?.code ?? null, reason: probe?.reason ?? null }),
      });
      continue;
    }
    try {
      const output = validateSemanticOutput(provider, provider.collect(context));
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
  createSemanticProviderRegistry({ providers });
  const results = [];
  for (const provider of providers) {
    try {
      const probe = await runProvider(provider, context, { signal, timeoutMs, allowProcess, operation: "probe" });
      const probeDisposition = dispositionForProbe(probe);
      if (["unsupported", "disabled"].includes(probeDisposition)) {
        results.push({ provider, probe, output: { nodes: [], edges: [], reports: [] }, disposition: typedDisposition(provider, probeDisposition, { code: probe?.code ?? null, reason: probe?.reason ?? null }) });
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
    const verification = liveVerificationCandidate(canonical, raw, sourceStateId);
    const evaluated = evaluateEvidence({
      targetSourceState: sourceStateId ?? canonical.sourceStateId ?? canonical.generationId ?? null,
      candidates: [canonical, verification],
      requestedRelation: canonical.relation ?? null,
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
