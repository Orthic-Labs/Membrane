import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
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

test("probeInstalled rejects non-installer paths instead of manufacturing installed identity", () => {
  const result = psh.probeInstalled({ cliPath: process.execPath });
  assert.equal(result.evidenceKind, "installed");
  assert.notEqual(result.status, "passed", "development/runtime executable must never qualify as installed");
  assert.match(String(result.reason || ""), /installed|status|current|release/i);
});

test("probeInstalled exposes stable-current release identity when installed root is usable", () => {
  const cliPath = "C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current/membrane.exe";
  if (!existsSync(cliPath)) return;
  const result = psh.probeInstalled({ cliPath });
  if (result.status !== "passed") {
    assert.equal(result.evidenceKind, "installed");
    assert.match(String(result.reason || ""), /installed|release|identity|current|status/i);
    return;
  }
  assert.equal(result.evidenceKind, "installed");
  assert.match(result.detail.identity.root, /[\\/]current$/iu);
  assert.equal(typeof result.detail.identity.version, "string");
  assert.match(result.detail.identity.releaseGeneration, /^sha256:[0-9a-f]{64}$/iu);
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

test("installed native Push rows use row-specific assertions & fail closed when Push surface is absent", async (t) => {
  const cliPath = "C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current/membrane.exe";
  if (!existsSync(cliPath)) return t.skip("stable installed membrane CLI is not present");
  const identity = psh.probeInstalled({ cliPath });
  if (identity.status !== "passed") return t.skip(`stable installed identity is not usable: ${identity.reason}`);

  const nativeRows = [
    "PSH_005", "PSH_008", "PSH_009", "PSH_010", "PSH_013", "PSH_016", "PSH_017",
    "PSH_019", "PSH_026", "PSH_027",
  ];
  const failureMarkers = /native|resolver|surface|receipt|prepare|telemetry|MCP|Push/iu;
  for (const id of nativeRows) {
    const result = await psh.CASES[id]({ cliPath });
    assert.equal(result.id, id.replace("_", "-"));
    assert.equal(result.group, "PSH");
    assert.equal(result.evidenceKind, "installed");
    assert.notEqual(result.status, "insufficient_implementation", `${id} must execute installed native probe, not permanent source-only insufficiency`);
    assert.ok(["passed", "failed", "blocked"].includes(result.status), `${id} returned invalid status ${result.status}`);
    if (result.status === "failed") assert.match(`${result.detail || ""} ${result.reason || ""}`, failureMarkers, `${id} failure must identify row-specific native condition`);
  }

  // Missing Push tools are an installed-runtime failure, never an
  // insufficient/source-only outcome. PSH-005 is first-row representative
  // for this surface contract & keeps this negative control bounded.
  const listed = spawnSync(cliPath, ["stdio-mcp"], {
    encoding: "utf8",
    windowsHide: true,
    timeout: 15_000,
    input: `${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-test", version: "1" } } })}\n${JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} })}\n`,
  });
  const responses = String(listed.stdout || "").split(/\r?\n/u).filter(Boolean).flatMap((line) => {
    try { return [JSON.parse(line)]; } catch { return []; }
  });
  const tools = responses.find((response) => response.id === 2)?.result?.tools;
  const pushSurfaceAvailable = Array.isArray(tools) && tools.some((tool) => tool.name === "membrane_push_prepare") && tools.some((tool) => tool.name === "membrane_push_resolve");
  const firstPushRow = await psh.CASES.PSH_005({ cliPath });
  assert.equal(firstPushRow.evidenceKind, "installed");
  if (!pushSurfaceAvailable) {
    assert.equal(firstPushRow.status, "failed");
    assert.match(`${firstPushRow.detail || ""} ${firstPushRow.reason || ""}`, /native Push|resolver|surface/iu);
  } else {
    assert.notEqual(firstPushRow.status, "insufficient_implementation");
  }
});
