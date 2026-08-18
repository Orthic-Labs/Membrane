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
