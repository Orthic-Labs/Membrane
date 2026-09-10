import assert from "node:assert/strict";
import test from "node:test";
import * as cases from "./bpt-windows.mjs";

// ---------------------------------------------------------------------------
// Contract shape: every BPT/BM/OPT export must be callable and must return
// { status, evidenceKind, reason } (see scripts/qualification/run.mjs
// runOneRegistryCase / EVIDENCE_KINDS) -- never undefined, never a bare
// boolean, and evidenceKind must be one of run.mjs's recognized set so the
// registry runner never silently drops a case.
// ---------------------------------------------------------------------------

const EVIDENCE_KINDS = new Set(["source", "component", "integration", "installed", "host", "task-outcome"]);

const REQUIRED_BPT_IDS = [
  "BPT-001", "BPT-002", "BPT-003", "BPT-004", "BPT-005", "BPT-006", "BPT-007", "BPT-008", "BPT-009", "BPT-010",
  "BPT-011", "BPT-012", "BPT-013", "BPT-014", "BPT-015", "BPT-016", "BPT-017", "BPT-018", "BPT-019", "BPT-020",
  "BPT-021", "BPT-023", "BPT-024", "BPT-025", "BPT-026", "BPT-027", "BPT-028", "BPT-029", "BPT-030", "BPT-031",
  "BPT-032", "BPT-033", "BPT-034", "BPT-035", "BPT-036", "BPT-037", "BPT-038", "BPT-039", "BPT-040", "BPT-041",
  "BPT-042", "BPT-043", "BPT-044", "BPT-046", "BPT-047", "BPT-049", "BPT-050", "BPT-051", "BPT-052", "BPT-053",
  "BPT-054", "BPT-055", "BPT-056", "BPT-057", "BPT-058", "BPT-059", "BPT-060", "BPT-061", "BPT-062", "BPT-063",
  "BPT-064", "BPT-065", "BPT-066", "BPT-067", "BPT-068", "BPT-069", "BPT-070", "BPT-071",
];
const REQUIRED_BM_IDS = ["BM03", "BM04", "BM05"];
const REQUIRED_OPT_IDS = ["OPT-01"];

test("every required BPT-xxx case export exists, is callable, and returns a well-formed outcome", async () => {
  for (const id of REQUIRED_BPT_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const outcome = await cases[exportName]();
    assert.ok(outcome && typeof outcome === "object", `${id}: no result object returned`);
    assert.ok(EVIDENCE_KINDS.has(outcome.evidenceKind), `${id}: evidenceKind ${outcome.evidenceKind} not in run.mjs EVIDENCE_KINDS`);
    assert.ok(typeof outcome.reason === "string" && outcome.reason.length > 0, `${id}: missing reason`);
    // Every result is typed; native source rows pass only when artifact exists.
    assert.notEqual(outcome.status, undefined, `${id}: missing status`);
  }
});

test("every required BM0x installed-native case export exists and is callable", () => {
  for (const id of REQUIRED_BM_IDS) {
    assert.ok(typeof cases[id] === "function", `missing export ${id}`);
    const outcome = cases[id]();
    assert.ok(outcome && typeof outcome === "object", `${id}: no result object returned`);
    assert.ok(EVIDENCE_KINDS.has(outcome.evidenceKind), `${id}: evidenceKind not recognized`);
    assert.ok(typeof outcome.reason === "string" && outcome.reason.length > 0, `${id}: missing reason`);
  }
});

test("OPT-01 exists, is callable, and never fabricates a pass ahead of its NCL-02/NCL-05 gate", () => {
  const outcome = cases.OPT_01();
  assert.ok(outcome && typeof outcome === "object");
  assert.equal(EVIDENCE_KINDS.has(outcome.evidenceKind), true);
  assert.notEqual(outcome.status, "passed", "OPT-01 must never report passed until NCL-02/NCL-05 pass");
});

test("BPT-002 uses native source evidence", async () => {
  const outcome = await cases.BPT_002();
  assert.equal(outcome.evidenceKind, "source");
  assert.equal(outcome.status, "passed");
});

// ---------------------------------------------------------------------------
// BM03/BM04/BM05 honesty check: these execute the REAL installed membrane.exe
// through isolated fixtures. This asserts the case module does not fall back
// to source markers or fabricate a pass when installed runtime is unavailable.
// ---------------------------------------------------------------------------

test("BM03/BM04/BM05 report insufficient (never a fabricated pass) if installed membrane.exe is unavailable", () => {
  const outcome = cases.BM03({ root: "/definitely/does/not/exist/on/this/machine" });
  assert.notEqual(outcome.status, "passed");
  assert.equal(outcome.evidenceKind, "installed");
  assert.match(outcome.reason, /installed native probe|MEMBRANE_QUALIFICATION_INSTALLED_ROOT|membrane\.exe/i);
});

// ---------------------------------------------------------------------------
// BM05 classifyRefusal: pure classifier for the fail-closed generation_mismatch
// refusal, isolated from process spawning. The installed CLI proves BM05 by
// refusing a mismatched generation with a typed error and non-zero exit --
// this must be recognized as the PASS condition, while a non-typed failure
// (wrong exit code, marker-less message, or an unexpected success) must still
// be rejected (negative control).
// ---------------------------------------------------------------------------

test("BM05 classifyRefusal: typed generation_mismatch refusal on non-zero exit is recognized", () => {
  const result = cases.classifyRefusal(1, "", "membrane: generation_mismatch: request generation does not match served generation", "generation_mismatch");
  assert.equal(result.failed, true);
  assert.equal(result.typed, true);
});

test("BM05 classifyRefusal negative control: non-zero exit WITHOUT the typed marker is not recognized as the proven outcome", () => {
  const result = cases.classifyRefusal(1, "", "membrane: internal_error: something else broke", "generation_mismatch");
  assert.equal(result.failed, true);
  assert.equal(result.typed, false, "a non-typed failure must never be classified as the fail-closed generation_mismatch proof");
});

test("BM05 classifyRefusal negative control: a zero exit code (unexpected success) is never classified as failed/typed", () => {
  const result = cases.classifyRefusal(0, "{\"state\":\"ok\"}", "", "generation_mismatch");
  assert.equal(result.failed, false);
  assert.equal(result.typed, false);
});

// ---------------------------------------------------------------------------
// BPT-020 native parity proof plus injected-fault negative controls.
// ---------------------------------------------------------------------------

test("BPT-020: native dependency DAG parity suite passes", async () => {
  const result = await cases.BPT_020();
  assert.equal(result.status, "passed", result.reason);
});

test("BPT-020 negative control: a full-rebuild fault (invalidatedProjections always returns every projection) fails", () => {
  const real = { PROJECTION_DEPENDENCIES: {
    bm25: ["source", "provider", "schema", "generation"],
    structural_search: ["source", "provider", "schema", "generation"],
    signatures: ["source", "provider", "schema", "generation"],
    contracts: ["source", "provider", "config", "schema", "generation"],
    processes: ["source", "provider", "config", "schema", "generation"],
    conventions: ["source", "config", "generation"],
    orientation: ["source", "provider", "config", "schema", "generation"],
  } };
  const faultyFullRebuild = {
    ...real,
    buildProjectionDependencyDag: (parents) => ({ parents }),
    // Fault: every parent change invalidates every projection (a full rebuild
    // masquerading as "selective" invalidation).
    invalidatedProjections: () => Object.keys(real.PROJECTION_DEPENDENCIES).sort(),
  };
  const result = cases.evaluateSelectiveInvalidation(faultyFullRebuild);
  assert.equal(result.pass, false, "a full-rebuild fault must be detected and must fail the selective-invalidation check");
});

test("BPT-020 negative control: a dropped config-edge fault (config change invalidates nothing) fails", () => {
  const real = { PROJECTION_DEPENDENCIES: {
    bm25: ["source", "provider", "schema", "generation"],
    structural_search: ["source", "provider", "schema", "generation"],
    signatures: ["source", "provider", "schema", "generation"],
    contracts: ["source", "provider", "config", "schema", "generation"],
    processes: ["source", "provider", "config", "schema", "generation"],
    conventions: ["source", "config", "generation"],
    orientation: ["source", "provider", "config", "schema", "generation"],
  } };
  const faultyDroppedConfig = {
    ...real,
    buildProjectionDependencyDag: (parents) => ({ parents }),
    // Fault: the `config` parent edge is silently dropped -- a real config
    // change never invalidates anything (the bug BPT-020 exists to catch).
    invalidatedProjections: (_dag, changedParents) => {
      const changed = new Set(changedParents);
      if (changed.has("config")) return [];
      return Object.entries(real.PROJECTION_DEPENDENCIES)
        .filter(([, deps]) => deps.some((d) => changed.has(d)))
        .map(([name]) => name)
        .sort();
    },
  };
  const result = cases.evaluateSelectiveInvalidation(faultyDroppedConfig);
  assert.equal(result.pass, false, "a dropped config-edge fault must be detected and must fail the selective-invalidation check");
});

// ---------------------------------------------------------------------------
// BPT-026 native parity proof plus compensatory comparator negative control.
// ---------------------------------------------------------------------------

test("BPT-026: native recall circuit parity suite passes", async () => {
  const result = await cases.BPT_026();
  assert.equal(result.status, "passed", result.reason);
});

test("BPT-026 negative control: a compensatory (summed-score) comparator that ignores hop-count fails", () => {
  // Fault: a compensatory scalar-sum comparator where a strong-enough
  // semantic-authority/tier score can outweigh hop-count entirely, so the
  // hop-count-only fixture pair never reorders (violating non-compensatory
  // ranking).
  const compensatoryComparator = (left, right) => {
    const score = (p) => p.semanticAuthorityRank * 1 + (p.hopCount === undefined ? 0 : 0); // hop-count deliberately not weighted
    return score(left) - score(right);
  };
  const result = cases.evaluateRankingComparator(compensatoryComparator);
  assert.equal(result.pass, false, "a comparator that ignores hop-count must fail the non-compensatory ranking fixture");
});

test("BPT-026 negative control: a comparator that is not a function fails", () => {
  const result = cases.evaluateRankingComparator(undefined);
  assert.equal(result.pass, false);
});
