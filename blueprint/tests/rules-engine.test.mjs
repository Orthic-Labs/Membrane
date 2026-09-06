// D41: rules engine — DSL parse, deterministic evaluation, exceptions, and
// baselines.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { parseRules } from "../src/lib/rules/parser.mjs";
import { evaluateRules, pathMatches } from "../src/lib/rules/evaluate.mjs";
import { isExceptionValid, suppressedByException } from "../src/lib/rules/exceptions.mjs";
import { changedSlice } from "../src/lib/rules/baseline.mjs";
import { FACT_PROVENANCE, isInferentialProvenance } from "../src/graph/provenance.mjs";

const EXAMPLE = join(import.meta.dirname, "..", "examples", "blueprint.rules.yml");

test("DSL parses the S-22 example", () => {
  const parsed = parseRules(readFileSync(EXAMPLE, "utf8"));
  assert.equal(parsed.rules.length, 1);
  const rule = parsed.rules[0];
  assert.equal(rule.id, "ui-must-not-import-db");
  assert.equal(rule.from.path, "src/ui/**");
  assert.deepEqual(rule.disallow.edgeKinds, ["IMPORTS", "CALLS"]);
  assert.equal(rule.disallow.to.path, "src/db/**");
  assert.equal(rule.severity, "error");
});

test("path globs match with ** and *", () => {
  assert.equal(pathMatches("src/ui/**", "src/ui/button.tsx"), true);
  assert.equal(pathMatches("src/ui/**", "src/db/conn.ts"), false);
  assert.equal(pathMatches("src/*/a.ts", "src/ui/a.ts"), true);
});

test("evaluation finds violations with evidence paths and fingerprints", () => {
  const parsed = parseRules(readFileSync(EXAMPLE, "utf8"));
  const findings = evaluateRules({
    rules: parsed.rules,
    edges: [
      { source: "src/ui/button.tsx", target: "src/db/conn.ts", kind: "IMPORTS", sourcePath: "src/ui/button.tsx", targetPath: "src/db/conn.ts" },
      { source: "src/lib/util.ts", target: "src/db/conn.ts", kind: "IMPORTS", sourcePath: "src/lib/util.ts", targetPath: "src/db/conn.ts" },
    ],
    providerVersions: {},
    generationId: "g1",
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].ruleId, "ui-must-not-import-db");
  assert.equal(findings[0].severity, "error");
  assert.ok(findings[0].fingerprint);
  assert.ok(findings[0].evidencePath.length === 2);
});

// BPT-011: a rule match is a declaration bound to evidence, never authority
// to assert an observed code fact. This mirrors the declared/observed split
// in src/graph/doc-truth-projection.mjs and reuses the shared provenance
// contract from src/graph/provenance.mjs.
test("rule findings are non-authoritative declarations bound to evidence, never observed graph facts", () => {
  const parsed = parseRules(readFileSync(EXAMPLE, "utf8"));
  const findings = evaluateRules({
    rules: parsed.rules,
    edges: [
      { source: "src/ui/button.tsx", target: "src/db/conn.ts", kind: "IMPORTS", sourcePath: "src/ui/button.tsx", targetPath: "src/db/conn.ts" },
    ],
    providerVersions: {},
    generationId: "g1",
  });
  assert.equal(findings.length, 1);
  const [finding] = findings;

  // Explicit non-authority tag: a finding cannot silently be mistaken for a
  // graph write or an observed code fact.
  assert.equal(finding.authority, "declaration");

  // Provenance is the categorical RULE_RESOLVED class, shared with the rest
  // of Blueprint's fact system — not a bespoke, ungoverned vocabulary.
  assert.equal(finding.provenance, FACT_PROVENANCE.RULE_RESOLVED);
  assert.equal(isInferentialProvenance(finding.provenance), false);

  // Categorical/deterministic facts carry confidence = null, never a
  // fabricated numeric probability, per the house provenance contract.
  assert.equal(finding.confidence, null);

  // The declaration is bound to the triggering evidence, not free-floating.
  assert.deepEqual(finding.declared, {
    ruleId: "ui-must-not-import-db",
    ruleVersion: "1.0.0",
    edgeKind: "IMPORTS",
    state: "violation",
  });
  assert.equal(finding.evidence.length, 2);
  assert.ok(finding.evidence.every((e) => e.kind === "code"));
  assert.equal(finding.evidence[0].path, "src/ui/button.tsx");
  assert.equal(finding.evidence[1].path, "src/db/conn.ts");

  // Detection itself is unchanged: same rule, same violated edge, same
  // fingerprint/evidencePath shape as before this envelope was added.
  assert.equal(finding.ruleId, "ui-must-not-import-db");
  assert.equal(finding.severity, "error");
  assert.deepEqual(finding.evidencePath, ["src/ui/button.tsx", "src/db/conn.ts"]);
});

test("fingerprints ignore line numbers (stable across rebuilds)", () => {
  const a = evaluateRules({ rules: [{ id: "r", from: { path: "a" }, disallow: { to: { path: "b" }, edgeKinds: [] }, severity: "error" }], edges: [{ source: "a", target: "b", kind: "X" }], providerVersions: {} });
  const b = evaluateRules({ rules: [{ id: "r", from: { path: "a" }, disallow: { to: { path: "b" }, edgeKinds: [] }, severity: "error" }], edges: [{ source: "a", target: "b", kind: "X" }], providerVersions: {} });
  assert.equal(a[0].fingerprint, b[0].fingerprint);
});

test("exceptions require owner/rationale/expiry and suppress only valid ones", () => {
  const now = Date.now();
  const valid = { owner: "adrian", rationale: "migration window", expires: new Date(now + 86400000).toISOString(), ruleId: "r1", scope: "global" };
  assert.equal(isExceptionValid(valid, { now }), true);
  assert.equal(isExceptionValid({ ...valid, owner: "" }, { now }), false);
  assert.equal(isExceptionValid({ ...valid, expires: new Date(now - 1000).toISOString() }, { now }), false);
  const finding = { ruleId: "r1", source: "src/x.ts" };
  assert.equal(suppressedByException(finding, [valid], { now }), true);
  assert.equal(suppressedByException(finding, [{ ...valid, ruleId: "other" }], { now }), false);
});

test("changed-slice baselines report only new violations with totals", () => {
  const baseline = { findings: [{ fingerprint: "f1" }] };
  const findings = [
    { fingerprint: "f1" },
    { fingerprint: "f2" },
  ];
  const slice = changedSlice({ findings, baseline });
  assert.equal(slice.total, 2);
  assert.equal(slice.newCount, 1);
  assert.equal(slice.retainedTotal, 2);
});
