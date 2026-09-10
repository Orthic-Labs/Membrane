import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as cases from "./adp-windows.mjs";

const { ALL_CASE_IDS, STRUCTURAL_CASE_IDS } = cases;
const BEHAVIOR_CASE_IDS = ["ADP-076", "ADP-077"];
const QUALIFIED_CASE_IDS = ALL_CASE_IDS.filter((id) => !BEHAVIOR_CASE_IDS.includes(id));

function exportNameFor(id) {
  return id.replace("-", "_");
}

// ---------------------------------------------------------------------------
// Positive path: every case in the packet's installedCaseIds set is
// exported, callable against the live repository without a build, and
// returns a well-formed typed result.
// ---------------------------------------------------------------------------

test("every non-service ADP case has source-bound installed identity evidence", () => {
  for (const id of QUALIFIED_CASE_IDS) {
    const exportName = exportNameFor(id);
    assert.ok(typeof cases[exportName] === "function", `missing export ${exportName}`);
    const result = cases[exportName]();
    assert.equal(result.id, id);
    assert.equal(result.kind, result.evidence?.native ? "installed" : "insufficient", `${id} must not claim a row-specific control it does not expose`);
    assert.ok(typeof result.pass === "boolean", `${id} must return a boolean pass field`);
    assert.ok(typeof result.reason === "string" && result.reason.length > 0, `${id} must return a reason`);
    assert.ok(typeof result.requirement === "string" && result.requirement.length > 0, `${id} must carry its requirement text`);
    if (result.evidence?.native) {
      assert.equal(result.status, "passed", `${id} native route failed: ${result.reason}`);
      assert.equal(result.pass, true);
    } else {
      assert.equal(result.status, "insufficient", `${id} changed canonical qualification status: ${result.reason}`);
      assert.equal(result.pass, false);
    }
    assert.equal(result.evidenceKind, "installed");
    assert.ok(result.evidence?.source?.evidence?.length > 0, `${id} omitted source digest evidence`);
    assert.equal(result.evidence?.identity?.runtimeOrigin, "installed");
  }
});

test("installed probe is an actual CLI check and fails closed when CLI is absent", () => {
  const result = cases.probeInstalled({ cliPath: "membrane-binary-that-does-not-exist-xyz" });
  assert.equal(result.status, "blocked");
  assert.equal(result.evidenceKind, "installed");
});

test("canonical COMPLETE rows retain source markers beside installed identity evidence", () => {
  for (const id of STRUCTURAL_CASE_IDS) {
    const result = cases[exportNameFor(id)]();
    if (result.evidence?.native) {
      assert.equal(result.kind, "installed");
      assert.equal(result.pass, true, `${id} row-specific native route failed`);
    } else {
      assert.equal(result.kind, "insufficient");
      assert.equal(result.pass, false, `${id} must not claim structural source as installed semantics`);
    }
    assert.ok(result.evidence?.source?.evidence?.length > 0);
  }
});

// ---------------------------------------------------------------------------
// Honesty invariant: no PARTIAL/BEHAVIORAL_REIMPLEMENT row is ever reported
// as a pass, and each carries a named gap. This is the module's core
// "never fabricate a pass for an unimplemented capability" guarantee.
// ---------------------------------------------------------------------------

test("partial rows preserve canonical gaps beside installed identity evidence", () => {
  const partialIds = QUALIFIED_CASE_IDS.filter((id) => !STRUCTURAL_CASE_IDS.includes(id));
  assert.ok(partialIds.length > 0);
  for (const id of partialIds) {
    const result = cases[exportNameFor(id)]();
    assert.equal(result.kind, "insufficient");
    assert.equal(result.pass, false, `${id} must preserve canonical partial status: ${result.reason}`);
    assert.ok(typeof result.evidence?.gap === "string" && result.evidence.gap.length > 0, `${id} must preserve canonical gap`);
  }
});

// ---------------------------------------------------------------------------
// Negative controls (uniform, executable, per SUBRULES.md hard rule 4):
// point every case at an isolated fixture root that does not contain the
// row's canonical implementation file(s) (simulating the capability's
// source being absent/reverted) and prove:
//   - every case fails closed with installed evidence when source is absent;
// Never touches the live repository: each case receives an explicit empty
// fixture root via its `{ root }` option.
// ---------------------------------------------------------------------------

test("negative control: every ADP source check fails closed when canonical files are absent", () => {
  const root = mkdtempSync(join(tmpdir(), "adp-windows-nc-"));
  try {
    for (const id of QUALIFIED_CASE_IDS) {
      const result = cases[exportNameFor(id)]({ root });
      assert.equal(result.kind, "installed", `${id} negative control changed kind unexpectedly`);
      assert.equal(result.pass, false, `${id} incorrectly passed against an empty fixture root with no canonical source present`);
      assert.ok(/source evidence unavailable|canonical implementation files exist/.test(result.reason), `${id} did not report the missing-source fault: ${result.reason}`);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("negative control: partial ADP rows never pass without source evidence", () => {
  const root = mkdtempSync(join(tmpdir(), "adp-windows-nc-insufficient-"));
  try {
    for (const id of QUALIFIED_CASE_IDS.filter((candidate) => !STRUCTURAL_CASE_IDS.includes(candidate))) {
      const result = cases[exportNameFor(id)]({ root });
      assert.equal(result.kind, "installed");
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
    assert.equal(result.kind, "installed");
    assert.equal(result.pass, false, "ADP-026 must not pass on a stub file lacking verify_seal/validate_envelope_mutation/batch_seal");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
