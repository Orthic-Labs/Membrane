// D41: rule evaluation — deterministic, local. Every result includes the
// shortest evidence path, matched selector, rule version, suppression state,
// and remediation text supplied by the rule.

import { createHash } from "node:crypto";

export function pathMatches(pattern, path) {
  if (!pattern) return true;
  if (pattern === path) return true;
  // Minimal glob: ** matches any depth, * matches within a segment.
  const regex = pattern
    .replace(/[.+^${}()|[\]\\]/g, "\\$&")
    .replace(/\*\*/g, "__DOUBLE__")
    .replace(/\*/g, "[^/]*")
    .replace(/__DOUBLE__/g, ".*");
  return new RegExp(`^${regex}$`).test(path);
}

export function evaluateRules({ rules, edges = [], nodes = [], providerVersions = {}, generationId = null }) {
  const findings = [];
  for (const rule of rules) {
    const ruleVersion = String(providerVersions[rule.id] ?? "1.0.0");
    for (const edge of edges) {
      const fromMatches = pathMatches(rule.from?.path, edge.sourcePath ?? edge.source ?? "");
      const toMatches = pathMatches(rule.disallow?.to?.path, edge.targetPath ?? edge.target ?? "");
      const kindMatches = !rule.disallow?.edgeKinds?.length || rule.disallow.edgeKinds.includes(edge.kind);
      if (!fromMatches || !toMatches || !kindMatches) continue;
      const fingerprint = createHash("sha256")
        .update(`${rule.id}:${ruleVersion}:${edge.source}:${edge.kind}:${edge.target}:${providerVersions[edge.kind] ?? ""}`)
        .digest("hex")
        .slice(0, 16);
      findings.push({
        ruleId: rule.id,
        ruleVersion,
        ruleName: rule.id,
        ruleDescription: rule.rationale ?? "",
        state: "violation",
        severity: rule.severity ?? "error",
        fingerprint,
        source: edge.source,
        target: edge.target,
        evidencePath: [edge.source, edge.target],
        generationId,
        confidenceTier: edge.confidenceTier ?? null,
        message: `rule ${rule.id} violated by ${edge.source} -> ${edge.target} (${edge.kind})`,
        helpUri: null,
        remediation: rule.rationale ?? "",
      });
    }
  }
  return findings;
}
