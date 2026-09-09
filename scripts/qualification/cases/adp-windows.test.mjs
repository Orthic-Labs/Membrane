import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as cases from "./adp-windows.mjs";

const { ALL_CASE_IDS, STRUCTURAL_CASE_IDS } = cases;
const INSUFFICIENT_CASE_IDS = ALL_CASE_IDS.filter((id) => !STRUCTURAL_CASE_IDS.includes(id));

function exportNameFor(id) {
  return id.replace("-", "_");
}

// ---------------------------------------------------------------------------
// Positive path: every case in the packet's installedCaseIds set is
// exported, callable against the live repository without a build, and
// returns a well-formed typed result.
// ---------------------------------------------------------------------------

test("every ADP-* case in the sub-lane packet is exported and callable", () => {
  for (const id of ALL_CASE_IDS) {
    const exportName = exportNameFor(id);
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    assert.ok(result.kind === "structural" || result.kind === "insufficient", `${id} has unexpected kind ${result.kind}`);
    assert.ok(typeof result.pass === "boolean", `${id} must return a boolean pass field`);
    assert.ok(typeof result.reason === "string" && result.reason.length > 0, `${id} must return a reason`);
    assert.ok(typeof result.requirement === "string" && result.requirement.length > 0, `${id} must carry its requirement text`);
  }
});

test("every structural ADP case passes against the live repository (canonical file + real marker present)", () => {
  for (const id of STRUCTURAL_CASE_IDS) {
    const result = cases[exportNameFor(id)]();
    assert.equal(result.kind, "structural");
    assert.equal(result.pass, true, `${id} unexpectedly failed structural attestation against the live tree: ${JSON.stringify(result)}`);
  }
});

// ---------------------------------------------------------------------------
// Honesty invariant: no PARTIAL/BEHAVIORAL_REIMPLEMENT row is ever reported
// as a pass, and each carries a named gap. This is the module's core
// "never fabricate a pass for an unimplemented capability" guarantee.
// ---------------------------------------------------------------------------

test("every insufficient ADP case reports pass:false with a named gap, never a fabricated pass", () => {
  assert.ok(INSUFFICIENT_CASE_IDS.length > 0, "expected at least one PARTIAL row in this packet");
  for (const id of INSUFFICIENT_CASE_IDS) {
    const result = cases[exportNameFor(id)]();
    assert.equal(result.kind, "insufficient");
    assert.equal(result.pass, false, `${id} must never report pass:true while PARTIAL`);
    assert.ok(typeof result.gap === "string" && result.gap.length > 0, `${id} must name its specific gap`);
  }
});

// ---------------------------------------------------------------------------
// Negative controls (uniform, executable, per SUBRULES.md hard rule 4):
// point every case at an isolated fixture root that does not contain the
// row's canonical implementation file(s) (simulating the capability's
// source being absent/reverted) and prove:
//   - structural cases fail closed: pass flips to false, kind stays
//     "structural" (never silently reclassified as a pass elsewhere), and
//     the reason names the missing file(s);
//   - insufficient cases stay honest: still pass:false, still kind
//     "insufficient", regardless of the injected fault — they never
//     opportunistically upgrade to a pass under any root.
// Never touches the live repository: each case receives an explicit empty
// fixture root via its `{ root }` option.
// ---------------------------------------------------------------------------

test("negative control: every structural ADP case fails closed when its canonical file is absent", () => {
  const root = mkdtempSync(join(tmpdir(), "adp-windows-nc-"));
  try {
    for (const id of STRUCTURAL_CASE_IDS) {
      const result = cases[exportNameFor(id)]({ root });
      assert.equal(result.kind, "structural", `${id} negative control changed kind unexpectedly`);
      assert.equal(result.pass, false, `${id} incorrectly passed against an empty fixture root with no canonical source present`);
      assert.ok(/none of the canonical implementation files exist/.test(result.reason), `${id} did not report the missing-file fault: ${result.reason}`);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: every insufficient ADP case still refuses to pass under the same injected fault", () => {
  const root = mkdtempSync(join(tmpdir(), "adp-windows-nc-insufficient-"));
  try {
    for (const id of INSUFFICIENT_CASE_IDS) {
      const result = cases[exportNameFor(id)]({ root });
      assert.equal(result.kind, "insufficient");
      assert.equal(result.pass, false, `${id} must remain pass:false under fault injection too`);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// Marker-drift control: a fixture root that has the canonical file present
// but with content that does not contain any of the case's expected
// markers must still fail (proves the check inspects real content, not
// just file existence).
// ---------------------------------------------------------------------------

test("negative control: a structural case fails when its file exists but carries none of the expected markers", () => {
  const root = mkdtempSync(join(tmpdir(), "adp-windows-nc-nomarker-"));
  try {
    const dir = join(root, "engine/crates/membrane-adapt/src");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "seal.rs"), "// empty stub, no real symbols\n");
    const result = cases.ADP_026({ root });
    assert.equal(result.kind, "structural");
    assert.equal(result.pass, false, "ADP-026 must not pass on a stub file lacking verify_seal/validate_envelope_mutation/batch_seal");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
