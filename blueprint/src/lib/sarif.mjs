// D42: SARIF conversion (S-23) — stable rule IDs, severities, fingerprints,
// paths, regions, help URIs, and evidence properties.

function uniqueRules(findings) {
  const byId = new Map();
  for (const finding of findings) {
    if (byId.has(finding.ruleId)) continue;
    byId.set(finding.ruleId, {
      id: finding.ruleId,
      name: finding.ruleName ?? finding.ruleId,
      shortDescription: { text: finding.ruleDescription ?? finding.message ?? finding.ruleId },
      helpUri: finding.helpUri ?? undefined,
      defaultConfiguration: {
        level: ({ error: "error", warning: "warning", info: "note" })[finding.severity] ?? "warning",
      },
    });
  }
  return [...byId.values()].sort((left, right) => left.id.localeCompare(right.id));
}

export function toSarif(findings, toolVersion) {
  return {
    version: "2.1.0",
    $schema: "https://json.schemastore.org/sarif-2.1.0.json",
    runs: [{
      tool: { driver: { name: "Blueprint", version: toolVersion, rules: uniqueRules(findings) } },
      results: findings.map((finding) => ({
        ruleId: finding.ruleId,
        level: ({ error: "error", warning: "warning", info: "note" })[finding.severity] ?? "warning",
        message: { text: finding.message },
        partialFingerprints: { blueprintFinding: finding.fingerprint },
        locations: [{ physicalLocation: {
          artifactLocation: { uri: finding.path },
          region: { startLine: finding.startLine ?? 1, endLine: finding.endLine ?? finding.startLine ?? 1 },
        }}],
        properties: {
          generationId: finding.generationId,
          confidenceTier: finding.confidenceTier,
          evidencePath: finding.evidencePath,
        },
      })),
    }],
  };
}
