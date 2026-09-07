import { createHash } from "node:crypto";

export const PROVIDER_PROTOCOL_VERSION = 1;
export const PROVIDER_KINDS = Object.freeze(["compiler", "structural", "framework", "schema", "bridge", "repository-evidence", "ranking", "provenance"]);
export const PROVIDER_FILESYSTEM_PERMISSIONS = Object.freeze(["none", "repo-read"]);
export const PROVIDER_NETWORK_PERMISSIONS = Object.freeze(["none"]);
export const PROVIDER_PROCESS_PERMISSIONS = Object.freeze(["none", "opt-in"]);

function providerError(code, message, details = undefined) {
  const error = new Error(message);
  error.code = code;
  if (details !== undefined) error.details = details;
  return error;
}

function stableValue(value) {
  if (Array.isArray(value)) return value.map(stableValue);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stableValue(value[key])]));
}

function stableJson(value) {
  return JSON.stringify(stableValue(value));
}

function canonicalCapabilities(capabilities) {
  if (!Array.isArray(capabilities)) throw providerError("provider_capabilities_invalid", "provider capabilities must be an array");
  const normalized = [...new Set(capabilities.map((value) => String(value).trim()).filter(Boolean))].sort();
  if (normalized.length !== capabilities.length) throw providerError("provider_capabilities_invalid", "provider capabilities must be non-empty unique strings");
  return normalized;
}

function canonicalPermissions(permissions) {
  if (!permissions || typeof permissions !== "object" || Array.isArray(permissions)) {
    throw providerError("provider_permissions_invalid", "provider permissions must be an object");
  }
  const normalized = {
    filesystem: String(permissions.filesystem ?? "repo-read"),
    network: String(permissions.network ?? "none"),
    process: String(permissions.process ?? "none"),
  };
  if (!PROVIDER_FILESYSTEM_PERMISSIONS.includes(normalized.filesystem)) {
    throw providerError("provider_permissions_invalid", `unsupported filesystem permission ${normalized.filesystem}`);
  }
  if (!PROVIDER_NETWORK_PERMISSIONS.includes(normalized.network)) {
    throw providerError("provider_permissions_invalid", `provider network permission must be none, got ${normalized.network}`);
  }
  if (!PROVIDER_PROCESS_PERMISSIONS.includes(normalized.process)) {
    throw providerError("provider_permissions_invalid", `unsupported process permission ${normalized.process}`);
  }
  return Object.freeze(normalized);
}

function validateProviderIdentity(provider) {
  if (!/^[a-z0-9][a-z0-9._-]*$/i.test(String(provider.id ?? ""))) {
    throw providerError("provider_id_invalid", `invalid provider id ${provider.id ?? ""}`);
  }
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(String(provider.version ?? ""))
      && !/^[a-z0-9][a-z0-9._-]*$/i.test(String(provider.version ?? ""))) {
    throw providerError("provider_version_invalid", `invalid provider version ${provider.version ?? ""}`);
  }
  if (!PROVIDER_KINDS.includes(String(provider.kind))) {
    throw providerError("provider_kind_invalid", `unknown provider kind ${provider.kind}`);
  }
  if (typeof provider.protocolRange !== "string" || !provider.protocolRange.trim()) {
    throw providerError("provider_protocol_invalid", "provider protocolRange must be a non-empty string");
  }
}

export function providerDescriptorDigest(provider) {
  const descriptor = {
    id: provider.id,
    version: provider.version,
    kind: provider.kind,
    protocolRange: provider.protocolRange,
    capabilities: [...(provider.capabilities ?? [])],
    permissions: provider.permissions ?? null,
  };
  return `sha256:${createHash("sha256").update(stableJson(descriptor)).digest("hex")}`;
}

export function validateProviderManifest(manifest, {
  artifactBytes = null,
  allowedLicenses = null,
} = {}) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw providerError("provider_manifest_invalid", "provider manifest must be an object");
  }
  for (const key of ["id", "version", "license", "integrity", "entry"]) {
    if (manifest[key] === undefined || manifest[key] === null || String(manifest[key]).trim() === "") {
      throw providerError("provider_manifest_invalid", `provider manifest missing ${key}`);
    }
  }
  const entry = String(manifest.entry).replaceAll("\\", "/");
  if (entry.startsWith("/") || /^[A-Za-z]:\//.test(entry) || entry.split("/").includes("..") || /^[a-z]+:\/\//i.test(entry)) {
    throw providerError("provider_manifest_entry_invalid", `provider entry must be repository-relative: ${manifest.entry}`);
  }
  if (!/^sha256:[0-9a-f]{64}$/i.test(String(manifest.integrity))) {
    throw providerError("provider_integrity_invalid", "provider integrity must be sha256:<64 hex>");
  }
  const license = String(manifest.license);
  if (Array.isArray(allowedLicenses) && allowedLicenses.length && !allowedLicenses.includes(license)) {
    throw providerError("provider_license_rejected", `provider license ${license} is not allowed`, { license });
  }
  if (artifactBytes !== null) {
    const bytes = Buffer.isBuffer(artifactBytes) ? artifactBytes : Buffer.from(String(artifactBytes));
    const observed = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
    if (observed.toLowerCase() !== String(manifest.integrity).toLowerCase()) {
      throw providerError("provider_integrity_mismatch", "provider artifact checksum does not match manifest", {
        expected: String(manifest.integrity), observed,
      });
    }
  }
  return Object.freeze({ ...manifest, entry, license, integrity: String(manifest.integrity).toLowerCase() });
}

export function defineProvider(provider) {
  for (const key of ["id", "version", "kind", "protocolRange", "capabilities", "permissions", "probe", "collect"]) {
    if (provider?.[key] === undefined) throw providerError("provider_contract_invalid", `provider missing ${key}`);
  }
  validateProviderIdentity(provider);
  if (typeof provider.probe !== "function" || typeof provider.collect !== "function") {
    throw providerError("provider_contract_invalid", "provider probe and collect must be functions");
  }
  const capabilities = Object.freeze(canonicalCapabilities(provider.capabilities));
  const permissions = canonicalPermissions(provider.permissions);
  const defined = {
    ...provider,
    id: String(provider.id),
    version: String(provider.version),
    kind: String(provider.kind),
    protocolRange: String(provider.protocolRange),
    capabilities,
    permissions,
  };
  return Object.freeze({ ...defined, descriptorDigest: providerDescriptorDigest(defined) });
}

export class ProviderRegistry {
  constructor({ allowedLicenses = [] } = {}) {
    this.allowedLicenses = Object.freeze([...allowedLicenses]);
    this.providers = new Map();
  }

  register(provider, { manifest = null, artifactBytes = null } = {}) {
    const defined = Object.isFrozen(provider) && provider.descriptorDigest ? provider : defineProvider(provider);
    if (this.providers.has(defined.id)) throw providerError("provider_duplicate", `provider ${defined.id} is already registered`);
    let validatedManifest = null;
    if (manifest) {
      validatedManifest = validateProviderManifest(manifest, { artifactBytes, allowedLicenses: this.allowedLicenses });
      if (validatedManifest.id !== defined.id || validatedManifest.version !== defined.version) {
        throw providerError("provider_manifest_identity_mismatch", "provider manifest identity does not match provider", {
          manifest: { id: validatedManifest.id, version: validatedManifest.version },
          provider: { id: defined.id, version: defined.version },
        });
      }
    }
    const record = Object.freeze({ provider: defined, manifest: validatedManifest });
    this.providers.set(defined.id, record);
    return record;
  }

  get(id) {
    return this.providers.get(String(id)) ?? null;
  }

  list() {
    return [...this.providers.values()].sort((left, right) => left.provider.id.localeCompare(right.provider.id));
  }

  capability(capability) {
    return this.list().filter(({ provider }) => provider.capabilities.includes(capability));
  }
}

function cancellationError() {
  return providerError("provider_cancelled", "provider execution cancelled");
}

/**
 * Admission bounds that do not depend on the call being asynchronous.
 *
 * Shared by `runProvider` and `runProviderSync` so there is exactly one place
 * that decides whether a provider may run at all: contract definition (which
 * rejects any permission value outside the declared enums), network refusal,
 * filesystem permission enum, and process opt-in gating.
 */
function admitProviderForExecution(provider, { allowProcess = false, signal = null } = {}) {
  const defined = Object.isFrozen(provider) && provider.descriptorDigest ? provider : defineProvider(provider);
  if (defined.permissions.network !== "none") throw providerError("provider_network_forbidden", "provider network access is forbidden");
  if (!PROVIDER_FILESYSTEM_PERMISSIONS.includes(defined.permissions.filesystem)) throw providerError("provider_filesystem_forbidden", "provider filesystem permission is invalid");
  if (defined.permissions.process === "opt-in" && !allowProcess) {
    throw providerError("provider_process_not_authorized", `provider ${defined.id} requires explicit process authorization`);
  }
  if (signal?.aborted) throw cancellationError();
  return defined;
}

/**
 * Synchronous bounded lane (BPT-012).
 *
 * The production build path (`buildGraphGeneration` -> ... ->
 * `augmentGenerationWithFirstPartyProviders`) is synchronous and has many
 * synchronous callers, so it cannot await `runProvider`. Without this, the
 * isolation contract was reachable only from the async lane, which no
 * production caller used: providers ran on real builds with none of the bounds
 * applied.
 *
 * ENFORCED here, identically to `runProvider`:
 *   - contract validation (permission values outside the declared enums are
 *     refused before any provider code runs);
 *   - network refusal (`permissions.network` must be "none");
 *   - filesystem permission enum ("none" | "repo-read");
 *   - process opt-in gating (`permissions.process === "opt-in"` needs an
 *     explicit `allowProcess`);
 *   - pre-flight cancellation (an already-aborted signal refuses the run);
 *   - typed crash wrapping (an untyped throw becomes `provider_crash`).
 *
 * STRUCTURALLY IMPOSSIBLE in a synchronous call, and deliberately NOT claimed:
 *   - timeout: a synchronous provider body holds the only thread; no timer can
 *     interrupt it, so a hang is a hang;
 *   - mid-flight cancellation: an abort raised while the provider body is
 *     running cannot be observed until it returns.
 * A provider that needs either bound must run in the async lane
 * (`collectSemanticEvidence` / `runProvider`), which is why both lanes exist.
 */
export function runProviderSync(provider, context = {}, {
  signal = null,
  allowProcess = false,
  operation = "collect",
} = {}) {
  const defined = admitProviderForExecution(provider, { allowProcess, signal });
  const method = operation === "probe" ? defined.probe : defined.collect;
  const safeContext = Object.freeze({ ...context, signal });
  try {
    const result = method(safeContext);
    if (result && typeof result.then === "function") {
      throw providerError("provider_sync_required", `provider ${defined.id} returned a promise on the synchronous lane`);
    }
    return result;
  } catch (error) {
    if (error?.code) throw error;
    throw providerError("provider_crash", `provider ${defined.id} failed: ${String(error?.message ?? error)}`, { cause: String(error?.message ?? error) });
  }
}

export async function runProvider(provider, context = {}, {
  signal = null,
  timeoutMs = 5000,
  allowProcess = false,
  operation = "collect",
} = {}) {
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 120000) {
    throw providerError("provider_timeout_invalid", "provider timeout must be an integer from 1 to 120000 ms");
  }
  const defined = admitProviderForExecution(provider, { allowProcess, signal });
  const method = operation === "probe" ? defined.probe : defined.collect;
  const safeContext = Object.freeze({ ...context, signal });
  let timer = null;
  let abortListener = null;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(providerError("provider_timeout", `provider ${defined.id} exceeded ${timeoutMs} ms`)), timeoutMs);
  });
  const cancelled = signal ? new Promise((_, reject) => {
    abortListener = () => reject(cancellationError());
    signal.addEventListener("abort", abortListener, { once: true });
  }) : null;
  try {
    const work = Promise.resolve().then(() => method(safeContext));
    const result = await Promise.race(cancelled ? [work, timeout, cancelled] : [work, timeout]);
    if (signal?.aborted) throw cancellationError();
    return result;
  } catch (error) {
    if (error?.code) throw error;
    throw providerError("provider_crash", `provider ${defined.id} failed: ${String(error?.message ?? error)}`, { cause: String(error?.message ?? error) });
  } finally {
    if (timer) clearTimeout(timer);
    if (signal && abortListener) signal.removeEventListener("abort", abortListener);
  }
}

export const example = defineProvider({
  id: "membrane.typescript",
  version: "1.0.0",
  kind: "compiler",
  protocolRange: ">=1 <2",
  capabilities: ["definitions", "references", "types"],
  permissions: { filesystem: "repo-read", network: "none", process: "opt-in" },
  async probe() { return { state: "available", details: {} }; },
  async collect() { return { nodes: [], edges: [], reports: [] }; },
});
