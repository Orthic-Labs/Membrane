// D41: rule evaluation — deterministic, local. Every result includes the
// shortest evidence path, matched selector, rule version, suppression state,
// and remediation text supplied by the rule.
//
// BPT-011: a rule match is a DECLARATION bound to the graph evidence that
// triggered it, never an observed code fact and never authority on its own.
// Findings carry FACT_PROVENANCE.RULE_RESOLVED (categorical, confidence
// forced null by src/graph/provenance.mjs) plus an explicit
// `authority: "declaration"` tag and cited evidence, mirroring the
// declared/observed split in src/graph/doc-truth-projection.mjs.

import { createHash } from "node:crypto";
import { withFactProvenance, FACT_PROVENANCE } from "../../graph/provenance.mjs";

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
      const evidence = [
        { kind: "code", path: edge.source, edgeKind: edge.kind, role: "from" },
        { kind: "code", path: edge.target, edgeKind: edge.kind, role: "to" },
      ];
      findings.push(withFactProvenance({
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
        // A rule match is a declaration bound to the evidence that triggered
        // it — never an observed code fact, never authority on its own.
        // Downstream consumers must not treat this as a graph write.
        authority: "declaration",
        declared: { ruleId: rule.id, ruleVersion, edgeKind: edge.kind, state: "violation" },
        evidence,
      }, FACT_PROVENANCE.RULE_RESOLVED, null));
    }
  }
  return findings;
}
