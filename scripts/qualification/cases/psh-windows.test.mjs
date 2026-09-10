import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readFileSync } from "node:fs";
import * as psh from "./psh-windows.mjs";

const EXPECTED_IDS = Array.from({ length: 29 }, (_, i) => `PSH_${String(i + 1).padStart(3, "0")}`);

test("psh-windows exports one case function per registry row (PSH-001..PSH-029)", () => {
  for (const id of EXPECTED_IDS) {
    assert.equal(typeof psh[id], "function", `${id} must be exported as a function`);
    assert.equal(typeof psh.CASES[id], "function", `${id} must be present in CASES`);
  }
  assert.deepEqual(Object.keys(psh.CASES).sort(), [...EXPECTED_IDS].sort());
});

test("windows-r5 registry resolves every PSH row to its exact module export", () => {
  const registryPath = "D:/Claude/review/windows-r5/windows-acceptance.json";
  assert.ok(existsSync(registryPath), "frozen windows-r5 registry must be present");
  const rows = JSON.parse(readFileSync(registryPath, "utf8")).cases.filter((row) => row.id.startsWith("PSH-"));
  assert.deepEqual(rows.map((row) => row.id), EXPECTED_IDS.map((id) => id.replace("_", "-")));
  for (const row of rows) {
    assert.equal(row.caseFile, "scripts/qualification/cases/psh-windows.mjs", row.id);
    assert.equal(row.caseExport, row.id.replace("-", "_"), row.id);
    assert.equal(typeof psh[row.caseExport], "function", `${row.id} export must be callable`);
  }
});

test("every case is executable without a reachable CLI and returns a typed, non-fabricated result", async () => {
  // No cliPath is supplied and MEMBRANE_CLI_PATH is unset in the test process,
  // so resolveCli() falls back to a bare "membrane" that is not guaranteed to
  // exist on this machine. Each case must still run to completion and must
  // never report "pass" when it could not actually exercise the CLI.
  const ctx = { cliPath: "membrane-binary-that-does-not-exist-xyz" };
  for (const [id, fn] of Object.entries(psh.CASES)) {
    const result = await fn(ctx);
    assert.ok(result && typeof result === "object", `${id} must return an object`);
    assert.equal(result.id, id.replace("_", "-"), `${id} must self-identify with its registry id`);
    assert.equal(result.group, "PSH");
    assert.ok(
      ["passed", "failed", "blocked", "insufficient_implementation"].includes(result.status),
      `${id} returned an unrecognized status: ${result.status}`,
    );
    // The unreachable-CLI fault must never be silently upgraded to "pass".
    assert.notEqual(result.status, "passed", `${id} must not report pass when its CLI dependency is unreachable`);
    assert.ok(typeof result.requirement === "string" && result.requirement.length > 0, `${id} must carry its verbatim requirement text`);
  }
});

test("runAll() runs every case and keys results by registry id", async () => {
  const ctx = { cliPath: "membrane-binary-that-does-not-exist-xyz" };
  const results = await psh.runAll(ctx);
  assert.deepEqual(Object.keys(results).sort(), EXPECTED_IDS.sort());
  for (const id of EXPECTED_IDS) {
    assert.equal(results[id].id, id.replace("_", "-"));
  }
});

test("insufficient_implementation outcomes cite the canonicalImplementationRow gap, never a bare pass", async () => {
  // Rows PSH-005, PSH-008 through PSH-010, PSH-013 through PSH-017, PSH-019,
  // PSH-023, PSH-025 through PSH-027 are PARTIAL/ADAPT/ORIGINAL in the frozen
  // windows-r5 registry (not DELIVERED). Their positive assertion must be
  // withheld with a cited gap rather than fabricated.
  const mustBeInsufficientEvenWithNoCliDependency = [
    "PSH_016", "PSH_017", "PSH_019", "PSH_023", "PSH_025", "PSH_026", "PSH_027",
  ];
  for (const id of mustBeInsufficientEvenWithNoCliDependency) {
    const result = await psh.CASES[id]({});
    if (id === "PSH_025" && result.status === "failed") {
      assert.match(result.detail, /enrollment|fixture|native/i);
      continue;
    }
    assert.equal(result.status, "insufficient_implementation", `${id} must report insufficient_implementation`);
    assert.ok(result.gap && result.gap.length > 0, `${id} must cite a specific implementation gap`);
  }
});

test("PSH-022 (DELIVERED: strict mr://anchor/ syntax) fails closed on every malformed negative control", async () => {
  // This exercises resolveCli()'s CLI-reachability probe honestly: when no
  // CLI is reachable the case must report "blocked", not "pass" — proving
  // the negative-control assertions are not vacuously satisfied.
  const unreachable = await psh.CASES.PSH_022({ cliPath: "membrane-binary-that-does-not-exist-xyz" });
  assert.equal(unreachable.status, "blocked");
  assert.match(unreachable.reason, /no installed .membrane. CLI reachable/);
});

test("cliReachable probes --version and resolveCli honors ctx.cliPath then MEMBRANE_CLI_PATH then default", () => {
  assert.equal(psh.resolveCli({ cliPath: "explicit-path" }), "explicit-path");
  const previous = process.env.MEMBRANE_CLI_PATH;
  try {
    delete process.env.MEMBRANE_CLI_PATH;
    assert.equal(psh.resolveCli({}), "membrane");
    process.env.MEMBRANE_CLI_PATH = "env-path";
    assert.equal(psh.resolveCli({}), "env-path");
  } finally {
    if (previous === undefined) delete process.env.MEMBRANE_CLI_PATH;
    else process.env.MEMBRANE_CLI_PATH = previous;
  }
});

test("installed probe executes a real --version check and fails closed when CLI is unreachable", () => {
  const result = psh.probeInstalled({ cliPath: "membrane-binary-that-does-not-exist-xyz" });
  assert.equal(result.status, "blocked");
  assert.equal(result.evidenceKind, "installed");
});

test("DELIVERED PSH rows pass through the installed native CLI with semantic controls", async () => {
  const cliPath = "C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current/membrane.exe";
  assert.ok(existsSync(cliPath), "stable installed membrane CLI must be present");
  for (const id of ["PSH_004", "PSH_012", "PSH_022"]) {
    const result = await psh.CASES[id]({ cliPath });
    assert.equal(result.id, id.replace("_", "-"));
    assert.equal(result.group, "PSH");
    assert.equal(result.status, "passed", `${id}: ${result.detail || result.reason}`);
    assert.equal(result.evidenceKind, "installed");
    assert.equal(typeof result.requirement, "string");
    assert.equal(typeof result.detail, "string");
  }
});

test("installed run keeps every non-DELIVERED PSH row explicitly open with its gap, except PSH-001 real fixture acceptance", async () => {
  const cliPath = "C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current/membrane.exe";
  assert.ok(existsSync(cliPath), "stable installed membrane CLI must be present");
  const delivered = new Set(["PSH_001", "PSH_004", "PSH_012", "PSH_022"]);
  for (const id of EXPECTED_IDS) {
    const result = await psh.CASES[id]({ cliPath });
    if (delivered.has(id)) {
      assert.equal(result.status, "passed", id);
    } else {
      if (id === "PSH_025" && result.status === "failed") {
        assert.match(result.detail, /enrollment|fixture|native/i);
      } else {
        assert.equal(result.status, "insufficient_implementation", id);
        assert.match(result.gap, new RegExp(`PSH-I${id.slice(4)}`));
      }
    }
  }
});
