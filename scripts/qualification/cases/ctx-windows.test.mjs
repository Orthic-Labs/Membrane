import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as cases from "./ctx-windows.mjs";

// ---------------------------------------------------------------------------
// Positive path: every required CTX case export exists and is callable
// against the live repository without a build.
// ---------------------------------------------------------------------------

const REQUIRED_CTX_IDS = [
  "CTX-001", "CTX-002", "CTX-003", "CTX-004", "CTX-005", "CTX-006", "CTX-007", "CTX-008", "CTX-009", "CTX-010",
  "CTX-011", "CTX-012", "CTX-013", "CTX-014", "CTX-015", "CTX-016", "CTX-017", "CTX-018", "CTX-019", "CTX-020",
  "CTX-021", "CTX-022", "CTX-023", "CTX-024", "CTX-025", "CTX-026", "CTX-027", "CTX-028", "CTX-029", "CTX-030",
  "CTX-031", "CTX-032", "CTX-034", "CTX-035", "CTX-036", "CTX-037", "CTX-038", "CTX-040", "CTX-041",
];

// Rows whose canonicalImplementationRow already records a documented
// residual (PARTIAL) — structural marker presence is still expected, the
// `note` just must carry the residual instead of claiming COMPLETE.
const PARTIAL_IDS = new Set(["CTX-001", "CTX-004", "CTX-010", "CTX-017", "CTX-023", "CTX-024", "CTX-031"]);

test("every required CTX case export exists and is callable against the live repository", () => {
  for (const id of REQUIRED_CTX_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    if (id === "CTX-018") {
      if (result.kind === "installed") {
        assert.equal(result.evidenceKind, "installed");
        assert.ok(["passed", "failed"].includes(result.status));
        assert.equal(result.pass, result.status === "passed");
        if (result.status === "passed") {
          assert.equal(result.detail?.saved, true);
          assert.equal(result.detail?.loaded, true);
          assert.equal(result.detail?.closed, true);
        }
      } else {
        assert.equal(result.kind, "structural");
        assert.equal(result.evidenceKind, "source");
      }
    } else {
      assert.equal(result.kind, "structural");
      assert.ok(Array.isArray(result.evidence) || typeof result.evidence === "object");
    }
    if (PARTIAL_IDS.has(id)) {
      assert.ok(typeof result.note === "string" && result.note.length > 0, `${id} is documented PARTIAL but carries no residual note`);
    }
  }
});

test("installed Cortex probe fails closed when stable CLI is unreachable", () => {
  const result = cases.probeInstalled({ cliPath: "membrane-binary-that-does-not-exist-xyz" });
  assert.equal(result.status, "blocked");
  assert.equal(result.evidenceKind, "installed");
});

test("installed Cortex identity probe fails closed outside installer-owned current root", () => {
  const result = cases.probeInstalledIdentity({ cliPath: process.execPath });
  assert.equal(result.status, "blocked");
  assert.equal(result.evidenceKind, "installed");
  assert.match(result.reason, /current root/i);
});

// ---------------------------------------------------------------------------
// Negative controls — structural checks fail when the canonical
// implementation file is absent, and fail when present but carrying none of
// the expected contract markers. Exercised against a representative sample
// spanning cortex-core, cortex-store and membrane-runtime file locations.
// ---------------------------------------------------------------------------

function makeFixtureRoot() {
  const root = mkdtempSync(join(tmpdir(), "ctx-windows-"));
  for (const dir of [
    "engine/crates/cortex-core/src",
    "engine/crates/cortex-store/src",
    "engine/crates/membrane-runtime/src",
  ]) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  return root;
}

const STRUCTURAL_SAMPLE = ["CTX_001", "CTX_008", "CTX_011", "CTX_017", "CTX_026", "CTX_040"];

for (const exportName of STRUCTURAL_SAMPLE) {
  test(`negative control: ${exportName} fails when its canonical implementation file(s) are absent`, () => {
    const root = makeFixtureRoot();
    try {
      const result = cases[exportName]({ root });
      assert.equal(result.pass, false, `${exportName} unexpectedly passed with no implementation files present`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("negative control: CTX-008 fails when temporal.rs exists but carries none of the required contract markers", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/cortex-store/src/temporal.rs"), "// empty stub, no contract\n", "utf8");
    const result = cases.CTX_008({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: CTX-017 fails when graph.rs omits the closed relation vocabulary", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/cortex-core/src/graph.rs"), "fn noop() {}\n", "utf8");
    const result = cases.CTX_017({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// BM06 — must never fabricate a pass; and each of its three
// negativeControls must be real, executable, and fail on its own injected
// fault (never all failing for the same reason).
// ---------------------------------------------------------------------------

test("BM06 reports installed baseline execution when current CLI is available", () => {
  const result = cases.BM06();
  assert.equal(result.id, "BM06");
  if (result.evidenceKind === "installed" && result.detail?.installed?.baselineAvailable) {
    assert.equal(result.pass, true);
  } else {
    assert.equal(result.pass, false);
    assert.equal(result.status, "insufficient");
  }
});

test("negative control: BM06 unrelated-query standing projection check fails when no baseline/standing marker exists", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane-runtime/src/memory_provider.rs"), "fn only_query_driven() {}\n", "utf8");
    const result = cases.BM06_unrelated_query_missing_standing_projection({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: BM06 arbitrary-text-as-preference scan fails when the anti-pattern is injected", () => {
  const root = makeFixtureRoot();
  try {
    const baseline = cases.BM06_arbitrary_text_as_authoritative_preference({ root });
    assert.equal(baseline.pass, true, "baseline (no fixture) unexpectedly failed");

    const file = join(root, "engine/crates/membrane-runtime/src/injected.rs");
    writeFileSync(file, "fn build() { let p = arbitrary_text_as_preference(note); }\n", "utf8");
    const faulted = cases.BM06_arbitrary_text_as_authoritative_preference({ root });
    assert.equal(faulted.pass, false, "did not detect injected arbitrary-text-as-preference fault");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: BM06 provider-query-driven-only check fails when the provider file lacks a baseline marker", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane-runtime/src/memory_provider.rs"), "fn respond_to_query() {}\n", "utf8");
    const result = cases.BM06_provider_query_driven_only({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// BM07 — must never fabricate a pass; and each of its six negativeControls
// must be real, executable, and fail on its own injected fault.
// ---------------------------------------------------------------------------

test("BM07 reports installed relation/proposal execution when current CLI is available", () => {
  const result = cases.BM07();
  assert.equal(result.id, "BM07");
  if (result.evidenceKind === "installed" && result.detail?.installed?.relationAvailable) {
    assert.equal(result.pass, true);
    assert.equal(result.status, "passed");
  } else {
    assert.equal(result.pass, false);
    assert.equal(result.status, "insufficient");
  }
});

test("BM07 reports source closure when installed boundary is unavailable", () => {
  const result = cases.BM07();
  if (result.pass) {
    assert.match(result.reason, /relation record\/list/);
  } else {
    assert.match(result.reason, /EpisodeProposalV1/);
    assert.match(result.reason, /propose_episode/);
    assert.match(result.reason, /evidence_relation_survives_process_restart/);
  }
});

const BM07_ANTI_PATTERN_CONTROLS = [
  {
    fn: cases.BM07_enrichment_retires_valid_fact,
    label: "enrichment retiring a valid fact (Z07)",
    content: "fn apply() { retire_fact_on_enrichment(record); }\n",
  },
  {
    fn: cases.BM07_derivation_presented_as_observation,
    label: "derivation presented as observation (Z07)",
    content: "fn record() { present_derivation_as_observation(value); }\n",
  },
  {
    fn: cases.BM07_proposal_bypasses_admission_gates,
    label: "proposal bypassing admission gates (Z08)",
    content: "fn admit() { bypass_admission(proposal); }\n",
  },
  {
    fn: cases.BM07_infrequent_retrieval_suppresses_constraint,
    label: "infrequent retrieval suppressing an applicable constraint (Z09)",
    content: "fn decay() { suppress_by_infrequent_retrieval(constraint); }\n",
  },
  {
    fn: cases.BM07_provider_quality_signal_overrides_sufficiency,
    label: "provider-local quality signal overriding Pull sufficiency (Z10)",
    content: "fn score() { quality_signal_overrides_sufficiency(signal); }\n",
  },
];

for (const control of BM07_ANTI_PATTERN_CONTROLS) {
  test(`negative control: BM07 scan for "${control.label}" fails when its anti-pattern is injected`, () => {
    const root = makeFixtureRoot();
    try {
      const baseline = control.fn({ root });
      assert.equal(baseline.pass, true, `baseline (no fixture) unexpectedly failed for: ${control.label}`);

      const file = join(root, "engine/crates/cortex-core/src/injected.rs");
      writeFileSync(file, control.content, "utf8");
      const faulted = control.fn({ root });
      assert.equal(faulted.pass, false, `did not detect injected fault for: ${control.label}`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("negative control: BM07 episode-proposal check fails when review.rs omits rejected_alternatives/final_reason", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/cortex-core/src/review.rs"), "fn propose() {}\n", "utf8");
    const result = cases.BM07_episode_proposal_missing_rejected_alternatives({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// OPT-02 — never fabricates a pass for the ablation itself; its
// negativeControl (ablation on a non-matched corpus is rejected) is real and
// fails on the injected fault (wrong corpus id).
// ---------------------------------------------------------------------------

test("OPT-02 refuses missing corpus instead of claiming fixture-only ablation", () => {
  const result = cases.OPT_02();
  assert.equal(result.pass, false);
  assert.equal(result.status, "blocked");
  assert.equal(result.id, "OPT-02");
  assert.match(result.reason, /pinned matched corpus|missing corpus/i);
});

test("negative control: OPT-02 rejects ablation requested against a non-matched corpus id", () => {
  const result = cases.OPT_02({ corpusId: "some-other-unrelated-corpus" });
  assert.equal(result.pass, false);
  assert.equal(result.kind, "exclusion");
  assert.equal(result.evidence.corpusId, "some-other-unrelated-corpus");
});

test("OPT-02 accepts the matched corpus id as the (still insufficient, unexecuted) target", () => {
  const result = cases.OPT_02({ corpusId: "cortex-matched-corpus-v1" });
  // Corpus id matches, but no pinned corpus was supplied, so execution is
  // refused rather than replaced with a source/fixture assertion.
  assert.equal(result.pass, false);
  assert.equal(result.status, "blocked");
  assert.match(result.reason, /pinned matched corpus|missing corpus/i);
});

test("OPT-02 executes all three installed native arms and binds release identity", () => {
  const root = mkdtempSync(join(tmpdir(), "opt02-native-"));
  try {
    const corpusPath = join(root, "matched.json");
    const corpusBytes = JSON.stringify({ corpusId: "cortex-matched-corpus-v1", pinned: true, cases: [{ query: "temporal preference" }] });
    writeFileSync(corpusPath, corpusBytes, "utf8");
    const mock = join(root, "mock-native.mjs");
    writeFileSync(mock, "const a=process.argv.slice(2); if(a[0]==='--version') console.log('Membrane 1.0'); else if(a.includes('build-info')) console.log(JSON.stringify({release_generation:'sha256:installed'})); else console.log(JSON.stringify({status:'passed',metrics:{recall:1,ranking:1,temporal:1,paraphrase:1,preference:1,contradiction:1,latency:1,startup:1,rss:1}}));", "utf8");
    const result = cases.OPT_02({ corpusPath, expectedCorpusSha256: createHash("sha256").update(corpusBytes).digest("hex"), cliPath: process.execPath, cliPrefix: [mock] });
    assert.equal(result.pass, true);
    assert.equal(result.status, "passed");
    assert.deepEqual(Object.keys(result.evidence.arms).sort(), ["hybrid", "lexical-only", "vector-only"]);
    assert.equal(result.evidence.installedReleaseGeneration, "sha256:installed");
  } finally { rmSync(root, { recursive: true, force: true }); }
});
