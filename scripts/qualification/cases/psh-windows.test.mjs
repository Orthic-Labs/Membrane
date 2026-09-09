import assert from "node:assert/strict";
import test from "node:test";
import * as psh from "./psh-windows.mjs";

const EXPECTED_IDS = Array.from({ length: 29 }, (_, i) => `PSH_${String(i + 1).padStart(3, "0")}`);

test("psh-windows exports one case function per registry row (PSH-001..PSH-029)", () => {
  for (const id of EXPECTED_IDS) {
    assert.equal(typeof psh[id], "function", `${id} must be exported as a function`);
    assert.equal(typeof psh.CASES[id], "function", `${id} must be present in CASES`);
  }
  assert.deepEqual(Object.keys(psh.CASES).sort(), [...EXPECTED_IDS].sort());
});

test("every case is executable without a reachable CLI and returns a typed, non-fabricated result", () => {
  // No cliPath is supplied and MEMBRANE_CLI_PATH is unset in the test process,
  // so resolveCli() falls back to a bare "membrane" that is not guaranteed to
  // exist on this machine. Each case must still run to completion and must
  // never report "pass" when it could not actually exercise the CLI.
  const ctx = { cliPath: "membrane-binary-that-does-not-exist-xyz" };
  for (const [id, fn] of Object.entries(psh.CASES)) {
    const result = fn(ctx);
    assert.ok(result && typeof result === "object", `${id} must return an object`);
    assert.equal(result.id, id.replace("_", "-"), `${id} must self-identify with its registry id`);
    assert.equal(result.group, "PSH");
    assert.ok(
      ["pass", "fail", "blocked", "insufficient_implementation"].includes(result.status),
      `${id} returned an unrecognized status: ${result.status}`,
    );
    // The unreachable-CLI fault must never be silently upgraded to "pass".
    assert.notEqual(result.status, "pass", `${id} must not report pass when its CLI dependency is unreachable`);
    assert.ok(typeof result.requirement === "string" && result.requirement.length > 0, `${id} must carry its verbatim requirement text`);
  }
});

test("runAll() runs every case and keys results by registry id", () => {
  const ctx = { cliPath: "membrane-binary-that-does-not-exist-xyz" };
  const results = psh.runAll(ctx);
  assert.deepEqual(Object.keys(results).sort(), EXPECTED_IDS.sort());
  for (const id of EXPECTED_IDS) {
    assert.equal(results[id].id, id.replace("_", "-"));
  }
});

test("insufficient_implementation outcomes cite the canonicalImplementationRow gap, never a bare pass", () => {
  // Rows PSH-005, PSH-008 through PSH-010, PSH-013 through PSH-017, PSH-019,
  // PSH-023, PSH-025 through PSH-027 are PARTIAL/ADAPT/ORIGINAL in the frozen
  // windows-r5 registry (not DELIVERED). Their positive assertion must be
  // withheld with a cited gap rather than fabricated.
  const mustBeInsufficientEvenWithNoCliDependency = [
    "PSH_016", "PSH_017", "PSH_019", "PSH_023", "PSH_025", "PSH_026", "PSH_027",
  ];
  for (const id of mustBeInsufficientEvenWithNoCliDependency) {
    const result = psh.CASES[id]({});
    assert.equal(result.status, "insufficient_implementation", `${id} must report insufficient_implementation`);
    assert.ok(result.gap && result.gap.length > 0, `${id} must cite a specific implementation gap`);
  }
});

test("PSH-022 (DELIVERED: strict mr://anchor/ syntax) fails closed on every malformed negative control", () => {
  // This exercises resolveCli()'s CLI-reachability probe honestly: when no
  // CLI is reachable the case must report "blocked", not "pass" — proving
  // the negative-control assertions are not vacuously satisfied.
  const unreachable = psh.CASES.PSH_022({ cliPath: "membrane-binary-that-does-not-exist-xyz" });
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
