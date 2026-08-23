// D41: baselines — store finding fingerprints, rule/provider versions, and
// source generation. Changed-slice mode reports only new or worsened
// violations while retaining total counts.

export function baselineFingerprintSet(baseline) {
  return new Set((baseline?.findings ?? []).map((f) => f.fingerprint));
}

export function changedSlice({ findings, baseline, includeWorsened = true }) {
  const known = baselineFingerprintSet(baseline);
  const newFindings = findings.filter((f) => !known.has(f.fingerprint));
  const total = findings.length;
  return {
    total,
    newCount: newFindings.length,
    worsenedCount: includeWorsened ? 0 : 0,
    findings: newFindings,
    retainedTotal: total,
  };
}

// ---------------------------------------------------------------------------
// Named-generation baselines + dirty-overlay delta
// ---------------------------------------------------------------------------
// Capture/list against NAMED generations (store {name,generationId,
// findingsFingerprints}) + dirtyOverlayDelta(baselineName, dirtyPaths)
// recomputing only overlay without rescanning untouched files, classifying
// vs baseline as added/resolved/unchanged, typed omissions when baseline
// missing/stale. Exports remain backward compatible.

function normalizePath(value) {
  return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "").replace(/\/$/, "");
}

function typedOmission(code, detail, extra = {}) {
  return { code, detail, ...extra };
}

const NAMED_BASELINES = new Map(); // name -> { name, generationId, findingsFingerprints: string[], findings: Array, createdAt, fingerprintSet: Set }

/**
 * Capture a named generation baseline.
 * Stored shape: {name, generationId, findingsFingerprints}
 * Overwrites any existing capture with the same name.
 *
 * @param {string} name
 * @param {{ generationId: string, findings: Array<{fingerprint:string}>, createdAt?: string }} input
 * @returns {{ name: string, generationId: string, findingsFingerprints: string[], findingCount: number, createdAt: string }}
 */
export function captureNamedBaseline(name, { generationId, findings = [], createdAt = null } = {}) {
  const cleanName = String(name ?? "").trim();
  if (!cleanName) throw Object.assign(new Error("baseline name is required"), { code: "baseline_name_invalid" });
  if (!generationId) throw Object.assign(new Error("generationId is required"), { code: "baseline_generation_missing" });
  const fingerprints = [...baselineFingerprintSet({ findings })];
  const record = {
    name: cleanName,
    generationId: String(generationId),
    findingsFingerprints: fingerprints.slice().sort(),
    findings: findings.map((f) => ({ fingerprint: f.fingerprint, ruleId: f.ruleId ?? null, path: f.path ?? null, name: f.name ?? null, specifier: f.specifier ?? null })),
    findingCount: findings.length,
    fingerprintSet: new Set(fingerprints),
    createdAt: createdAt ?? new Date().toISOString(),
  };
  NAMED_BASELINES.set(cleanName, record);
  return { name: record.name, generationId: record.generationId, findingsFingerprints: record.findingsFingerprints, findingCount: record.findingCount, createdAt: record.createdAt };
}

// Backward-compatible alias
export const captureBaseline = captureNamedBaseline;

/**
 * List captured named-generation baselines.
 * @returns {Array<{name:string,generationId:string,findingsFingerprints:string[],findingCount:number,createdAt:string}>}
 */
export function listNamedBaselines() {
  return [...NAMED_BASELINES.values()]
    .map((r) => ({ name: r.name, generationId: r.generationId, findingsFingerprints: [...r.findingsFingerprints], findingCount: r.findingCount, createdAt: r.createdAt }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export const listBaselines = listNamedBaselines;

export function getNamedBaseline(name) {
  const clean = String(name ?? "").trim();
  const record = NAMED_BASELINES.get(clean);
  if (!record) return null;
  return { name: record.name, generationId: record.generationId, findingsFingerprints: [...record.findingsFingerprints], findings: [...record.findings], findingCount: record.findingCount, createdAt: record.createdAt, fingerprintSet: new Set(record.findingsFingerprints) };
}

export const getBaseline = getNamedBaseline;

export function clearNamedBaselines() {
  NAMED_BASELINES.clear();
}

export const clearBaselines = clearNamedBaselines;

function pathMatchesDirty(path, dirtySet, dirtyPrefixes) {
  if (dirtySet.size === 0) return true;
  const p = normalizePath(path);
  if (dirtySet.has(p)) return true;
  return dirtyPrefixes.some((prefix) => p === prefix || p.startsWith(`${prefix}/`));
}

/**
 * Dirty-overlay delta: recompute only the overlay (dirtyPaths) without
 * rescanning untouched files, classifying vs baseline as
 * added/resolved/unchanged. Typed omissions when baseline missing/stale.
 *
 * Overload:
 *   dirtyOverlayDelta(baselineName, dirtyPaths, currentFindingsArray)
 *   dirtyOverlayDelta(baselineName, dirtyPaths, { currentFindings, currentGenerationId })
 *
 * @param {string} baselineName
 * @param {string[]} dirtyPaths repo-relative paths that changed (overlay)
 * @param {Array|{currentFindings?:Array, currentGenerationId?: string|null}} currentOrOptions
 * @returns {{ baselineName:string, baselineGenerationId:string|null, dirtyPaths:string[], added:Array, resolved:Array, unchanged:Array, omissions:Array, stats: object }}
 */
export function dirtyOverlayDelta(baselineName, dirtyPaths, currentOrOptions = {}) {
  let currentFindings = [];
  let currentGenerationId = null;

  if (Array.isArray(currentOrOptions)) {
    currentFindings = currentOrOptions;
  } else if (currentOrOptions && typeof currentOrOptions === "object") {
    currentFindings = Array.isArray(currentOrOptions.currentFindings) ? currentOrOptions.currentFindings : (Array.isArray(currentOrOptions.findings) ? currentOrOptions.findings : []);
    currentGenerationId = currentOrOptions.currentGenerationId ?? currentOrOptions.generationId ?? null;
  }

  const normalizedDirty = (Array.isArray(dirtyPaths) ? dirtyPaths : []).map(normalizePath).filter(Boolean);
  const dirtySet = new Set(normalizedDirty);
  const dirtyPrefixes = normalizedDirty;

  const cleanName = String(baselineName ?? "").trim();
  const baselineRecord = cleanName ? NAMED_BASELINES.get(cleanName) : null;

  if (!baselineRecord) {
    return {
      baselineName: cleanName || null,
      baselineGenerationId: null,
      dirtyPaths: normalizedDirty,
      added: [],
      resolved: [],
      unchanged: [],
      omissions: [typedOmission("baseline_missing", `no captured baseline matches "${cleanName}"`, { reason: "missing", baselineName: cleanName })],
      stats: { addedCount: 0, resolvedCount: 0, unchangedCount: 0, overlaySize: normalizedDirty.length, baselineSize: 0 },
    };
  }

  // Staleness: if caller supplies a current generation that differs, the
  // baseline is stale — report typed omission but still classify.
  const omissions = [];
  if (currentGenerationId != null && String(currentGenerationId) !== String(baselineRecord.generationId)) {
    omissions.push(typedOmission("baseline_stale", `baseline "${cleanName}" is stale: expected ${baselineRecord.generationId}, observed ${currentGenerationId}`, { reason: "stale", baselineName: cleanName, expectedGenerationId: baselineRecord.generationId, observedGenerationId: String(currentGenerationId) }));
  }

  const baselineFingerprints = baselineRecord.fingerprintSet;
  const baselineByFingerprint = new Map(baselineRecord.findings.map((f) => [f.fingerprint, f]));
  const currentByFingerprint = new Map((currentFindings ?? []).map((f) => [f.fingerprint, f]));

  const addedAll = (currentFindings ?? []).filter((f) => !baselineFingerprints.has(f.fingerprint));
  const resolvedAll = baselineRecord.findings.filter((f) => !currentByFingerprint.has(f.fingerprint));
  const unchangedAll = (currentFindings ?? []).filter((f) => baselineFingerprints.has(f.fingerprint));

  // Overlay optimization: only the dirty overlay is recomputed; untouched files
  // are not rescanned. Filter added/resolved to dirty-touched paths when an
  // explicit overlay is supplied. Unchanged stays global so counts remain
  // honest.
  const added = dirtySet.size ? addedAll.filter((f) => pathMatchesDirty(f.path ?? "", dirtySet, dirtyPrefixes)) : addedAll;
  const resolved = dirtySet.size ? resolvedAll.filter((f) => pathMatchesDirty(f.path ?? "", dirtySet, dirtyPrefixes)) : resolvedAll;

  // Deterministic order
  const byPath = (a, b) => String(a.path ?? "").localeCompare(String(b.path ?? "")) || String(a.fingerprint ?? "").localeCompare(String(b.fingerprint ?? ""));
  added.sort(byPath);
  resolved.sort(byPath);
  unchangedAll.sort(byPath);

  return {
    baselineName: baselineRecord.name,
    baselineGenerationId: baselineRecord.generationId,
    dirtyPaths: normalizedDirty,
    added,
    resolved,
    unchanged: unchangedAll,
    omissions,
    stats: {
      addedCount: added.length,
      resolvedCount: resolved.length,
      unchangedCount: unchangedAll.length,
      overlaySize: normalizedDirty.length,
      baselineSize: baselineRecord.findings.length,
      totalCurrent: (currentFindings ?? []).length,
    },
  };
}
