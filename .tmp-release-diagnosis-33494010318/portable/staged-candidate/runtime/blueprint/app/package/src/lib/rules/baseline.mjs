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
// Named-generation baselines
// ---------------------------------------------------------------------------
// Capture/list against NAMED generations (store {name,generationId,
// findingsFingerprints}). Exports remain backward compatible.

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
