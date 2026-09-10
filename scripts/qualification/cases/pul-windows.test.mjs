import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as cases from "./pul-windows.mjs";

// ---------------------------------------------------------------------------
// Positive path: every exported case runs today against the live repository
// without a build, and the required PUL/EX exports exist.
// ---------------------------------------------------------------------------

const REQUIRED_PUL_IDS = [
  "PUL-001", "PUL-002", "PUL-003", "PUL-004", "PUL-005", "PUL-006", "PUL-007", "PUL-008", "PUL-009", "PUL-010",
  "PUL-011", "PUL-012", "PUL-013", "PUL-014", "PUL-015", "PUL-016", "PUL-017", "PUL-018", "PUL-019", "PUL-020",
  "PUL-021", "PUL-022", "PUL-023", "PUL-024", "PUL-025", "PUL-026", "PUL-027", "PUL-028", "PUL-029", "PUL-030",
  "PUL-031", "PUL-032", "PUL-033", "PUL-035", "PUL-036", "PUL-037", "PUL-039", "PUL-040", "PUL-041", "PUL-042",
];
const REQUIRED_EX_IDS = ["EX-01", "EX-02", "EX-03", "EX-04", "EX-05", "EX-06", "EX-07", "EX-08", "EX-09"];

test("every required PUL case export exists and is callable against the live repository", () => {
  for (const id of REQUIRED_PUL_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    assert.equal(result.kind, "structural");
    assert.equal(result.evidenceKind, "source");
    assert.equal(result.status, "passed");
    assert.ok(Array.isArray(result.evidence) || typeof result.evidence === "object");
  }
});

test("every required EX exclusion check exists, runs, and passes against the real live source tree", () => {
  for (const id of REQUIRED_EX_IDS) {
    const exportName = id.replace("-", "_");
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    assert.equal(result.kind, "exclusion");
    assert.ok(result.scannedFiles > 0, `EX check ${id} scanned zero files — scan roots likely wrong`);
    assert.equal(result.pass, true, `${id} unexpectedly found a forbidden pattern in the live tree: ${JSON.stringify(result.evidence)}`);
  }
});

// ---------------------------------------------------------------------------
// Negative controls — EX-01..EX-09 (windows-amendment-acceptance.json).
// Each injects the forbidden fixture into an isolated temp root (never the
// live repository) and asserts the check fails and reports the offending
// file.
// ---------------------------------------------------------------------------

function makeFixtureRoot() {
  const root = mkdtempSync(join(tmpdir(), "pul-windows-ex-"));
  for (const dir of [
    "engine/crates/membrane-core/src",
    "engine/crates/membrane-federation/src",
    "engine/crates/membrane-runtime/src/pull",
    "engine/crates/membrane-protocol/src",
  ]) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  return root;
}

const EX_NEGATIVE_CONTROLS = [
  { id: "EX-01", fn: cases.EX_01, file: "engine/crates/membrane-federation/src/injected.rs", content: "struct DreamPlanner;" },
  { id: "EX-02", fn: cases.EX_02, file: "engine/crates/membrane-core/src/injected.rs", content: "struct ClientSemanticAuthority;" },
  { id: "EX-03", fn: cases.EX_03, file: "engine/crates/membrane-federation/src/injected.rs", content: "fn mutate_graph(g: &mut Graph) {}" },
  { id: "EX-04", fn: cases.EX_04, file: "engine/crates/membrane-core/src/injected.rs", content: "struct MarkdownGraphStore;" },
  { id: "EX-05", fn: cases.EX_05, file: "engine/crates/membrane-runtime/src/pull/injected.rs", content: "fn admit_unverified_llm(x: &str) {}" },
  { id: "EX-06", fn: cases.EX_06, file: "engine/crates/membrane-federation/src/injected.rs", content: "let scalar_trust: f64 = 0.9;" },
  { id: "EX-07", fn: cases.EX_07, file: "engine/crates/membrane-core/src/injected.rs", content: "fn expire_truth_after(d: Duration) {}" },
  { id: "EX-08", fn: cases.EX_08, file: "engine/crates/membrane-runtime/src/pull/injected.rs", content: "fn speculative_merge(a: &Branch, b: &Branch) {}" },
  { id: "EX-09", fn: cases.EX_09, file: "engine/crates/membrane-protocol/src/injected.rs", content: "fn auto_paid_refresh() {}" },
];

for (const control of EX_NEGATIVE_CONTROLS) {
  test(`negative control: ${control.id} fails when its forbidden fixture is injected`, () => {
    const root = makeFixtureRoot();
    try {
      // Baseline: an empty fixture tree passes (nothing forbidden present).
      const baseline = control.fn({ root });
      assert.equal(baseline.pass, true, `${control.id} baseline (no fixture) unexpectedly failed`);

      writeFileSync(join(root, control.file), control.content, "utf8");
      const faulted = control.fn({ root });
      assert.equal(faulted.pass, false, `${control.id} did not detect its injected fault`);
      assert.ok(
        faulted.evidence.some((e) => e.file === control.file),
        `${control.id} failed but did not attribute the offending file`,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

// ---------------------------------------------------------------------------
// Negative controls — PUL structural checks: a fixture root with the
// canonical implementation file missing entirely, and one with the file
// present but empty (no contract marker), both must fail.
// ---------------------------------------------------------------------------

test("negative control: PUL-001 fails when engine/crates/membrane-federation/src/request.rs is absent", () => {
  const root = makeFixtureRoot();
  try {
    const result = cases.PUL_001({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: PUL-001 fails when request.rs exists but carries none of the required contract markers", () => {
  const root = makeFixtureRoot();
  try {
    writeFileSync(join(root, "engine/crates/membrane-federation/src/request.rs"), "// empty stub, no contract\n", "utf8");
    const result = cases.PUL_001({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: PUL-033 fails when fence_abstention.rs omits insufficient_confidence", () => {
  const root = makeFixtureRoot();
  try {
    mkdirSync(join(root, "engine/crates/membrane-federation/tests"), { recursive: true });
    writeFileSync(join(root, "engine/crates/membrane-federation/tests/fence_abstention.rs"), "fn noop() {}\n", "utf8");
    const result = cases.PUL_033({ root });
    assert.equal(result.pass, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
