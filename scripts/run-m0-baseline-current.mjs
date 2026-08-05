#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const membraneRoot = resolve(HERE, "..");
const workspaceRoot = resolve(membraneRoot, "..");
// Was "docs/plans/sol/membrane-competitive-review/contracts/..." — that directory no longer
// exists after the 2026-08-02 Roundtable-to-Citadel rename (commit 59f87213), which moved the
// fixture file to "docs/plans/sol/contracts/". The stale path made every invocation of this
// runner throw ENOENT before it ever executed a single probe or suite — the F13 defect was not
// merely "descriptive fixtures with a stub runner", it was a runner that could not run at all.
const fixturePath = join(workspaceRoot, "docs/plans/sol/contracts/fable-m0-false-success-fixtures.json");
const sha = (value) => createHash("sha256").update(value).digest("hex");

// Honesty disclosure (read this before trusting `baseline_red`): the `baseline` value below for
// every case is a literal transcription of the fixture's own `failure` column — the documented
// pre-fix input — not bytes read from an actual checkout of a historical commit. Reproducing the
// real historical behavior would require checking out an old commit of 5 different
// language runtimes (Rust engine, JS membrane/cortex, Python forge/morph) and rebuilding each,
// which this workspace's primary-checkout-only / no-worktree-without-approval rule forbids doing
// in place, and which a temporary worktree could only do with Adrian's explicit sign-off. So
// `baseline_red` here proves "the documented failure input fails the current validator", not
// "the historical commit actually produced this input" — every case is listed in
// `undrivable_fixtures` in the final report for exactly this reason. What IS live and unfabricated
// is `current_green`: it requires both the literal current-shape check below AND a fresh run, in
// this process, of the real test suite that owns the fix (see `suites` and `SOURCE_PROOF`-style
// owner mapping below) — a suite failure fails the case regardless of what the literal says.

const probes = {
  "mcp-write-noop": {
    baseline: { durable_id: "", readback_digest: "", state_changed: false },
    current: { durable_id: "proposal-1", readback_digest: `sha256:${sha("proposal-1")}`, state_changed: true },
    valid: (value) => Boolean(value.durable_id && /^sha256:[a-f0-9]{64}$/.test(value.readback_digest) && value.state_changed),
  },
  "hook-no-delivery": {
    baseline: { phases: ["hook.started", "hook.terminal"] },
    current: { phases: ["hook.started", "block.delivered", "hook.terminal"] },
    valid: (value) => value.phases.includes("block.delivered"),
  },
  "root-child-scope": {
    baseline: { requested: "repo-child", grants: ["repo-root"] },
    current: { requested: "repo-child", grants: ["repo-root", "repo-child"] },
    valid: (value) => value.grants.includes(value.requested),
  },
  "whole-graph-invalidation": {
    baseline: { changed_files: 1, invalidated_files: 300, bounded_delta: false },
    current: { changed_files: 1, invalidated_files: 1, bounded_delta: true },
    valid: (value) => value.bounded_delta && value.invalidated_files <= value.changed_files,
  },
  "generation-envelope-drift": {
    baseline: { rows: "current rows", generation_digest: sha("stale rows") },
    current: { rows: "current rows", generation_digest: sha("current rows") },
    valid: (value) => sha(value.rows) === value.generation_digest,
  },
  "arbitrary-shell-check": {
    baseline: { command: "true", expected_command: "cargo test", matched: false },
    current: { command: "cargo test", expected_command: "cargo test", matched: true },
    valid: (value) => value.matched && value.command === value.expected_command,
  },
  "morph-update-as-add": {
    baseline: { existing_id: "rule-1", resulting_ids: ["rule-1", "rule-2"] },
    current: { existing_id: "rule-1", resulting_ids: ["rule-1"] },
    valid: (value) => value.resulting_ids.length === 1 && value.resulting_ids[0] === value.existing_id,
  },
  "morph-field-loss": {
    baseline: { before: { authority: "A1", source_refs: ["source-1"] }, after: { authority: "A1" } },
    current: { before: { authority: "A1", source_refs: ["source-1"] }, after: { authority: "A1", source_refs: ["source-1"] } },
    valid: (value) => JSON.stringify(value.before) === JSON.stringify(value.after),
  },
  "forge-mcp-output-schema": {
    baseline: { structured_valid: false },
    current: { structured_valid: true },
    valid: (value) => value.structured_valid === true,
  },
  "overlay-without-session": {
    baseline: { session_id: "", worktree_id: "" },
    current: { session_id: "session-1", worktree_id: "primary" },
    valid: (value) => Boolean(value.session_id && value.worktree_id),
  },
};

const suites = {
  crypt: [process.execPath, ["--test", join(membraneRoot, "mcp/server-durable.test.mjs")], workspaceRoot],
  "membrane-host": [process.execPath, ["--test", join(membraneRoot, "mcp/client.test.mjs"), join(membraneRoot, "mcp/adapters.test.mjs")], workspaceRoot],
  // root-child-scope's fix lives in repository-catalog.mjs ("child graph access requires an
  // explicit root grant"), not project-registry.mjs — the original suite list omitted it, so
  // "current_green" for that fixture was gated on a suite that never actually exercised the fix.
  membrane: [process.execPath, ["--test", join(membraneRoot, "mcp/project-registry.test.mjs"), join(membraneRoot, "mcp/repository-catalog.test.mjs")], workspaceRoot],
  cortex: ["pnpm", ["--dir", join(workspaceRoot, "cortex"), "test:all"], workspaceRoot],
  forge: ["pnpm", ["--dir", join(workspaceRoot, "forge"), "test"], workspaceRoot],
  morph: [join(workspaceRoot, ".venv-tools/bin/python"), ["-m", "pytest", join(workspaceRoot, "morph/tests")], workspaceRoot],
};

function runSuite(owner) {
  const [command, args, cwd] = suites[owner];
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    timeout: 20 * 60_000,
    windowsHide: true,
    env: {
      ...process.env,
      CI: "1",
      CRYPT_TEST_BIN: process.env.CRYPT_TEST_BIN || join(membraneRoot, "engine/target/debug", process.platform === "win32" ? "crypt.exe" : "crypt"),
    },
  });
  const output = `${result.stdout || ""}${result.stderr || ""}`;
  return { owner, status: result.status, signal: result.signal || null, output_sha256: `sha256:${sha(output)}`, output_tail: output.slice(-1200) };
}

export { fixturePath, probes };

export function runM0({ runSuiteImpl = runSuite } = {}) {
  const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));
  if (fixture.schema !== "orthic.membrane.fable-m0-fixtures.v1") throw new Error("m0_fixture_schema_invalid");
  const unknown = fixture.cases.filter(({ id }) => !probes[id]).map(({ id }) => id);
  if (unknown.length) throw new Error(`m0_probe_missing:${unknown.join(",")}`);
  const owners = [...new Set(fixture.cases.map(({ owner }) => owner))];
  const suiteResults = Object.fromEntries(owners.map((owner) => [owner, runSuiteImpl(owner)]));
  const cases = fixture.cases.map((entry) => {
    const probe = probes[entry.id];
    const baselineFailed = !probe.valid(probe.baseline);
    const suitePassed = suiteResults[entry.owner].status === 0;
    const currentPassed = probe.valid(probe.current) && suitePassed;
    return {
      id: entry.id,
      owner: entry.owner,
      expected_reason: entry.expected,
      baseline: {
        status: baselineFailed ? "expected_failure" : "unexpected_pass",
        reason: baselineFailed ? entry.expected : "detector_did_not_reproduce",
        evidence_basis: "fixture_failure_description",
        live_reproduced: false,
      },
      current: {
        status: currentPassed ? "pass" : "fail",
        suite: entry.owner,
        suite_exit_status: suiteResults[entry.owner].status,
        evidence_basis: "live_current_head_test_suite",
        live_reproduced: true,
      },
    };
  });
  const passed = cases.every((entry) => entry.baseline.status === "expected_failure" && entry.current.status === "pass");
  return {
    schema: "orthic.membrane.m0-run.v1",
    passed,
    baseline_red: cases.filter((entry) => entry.baseline.status === "expected_failure").length,
    current_green: cases.filter((entry) => entry.current.status === "pass").length,
    cases,
    suites: Object.values(suiteResults),
    // Every case's baseline side is a documented replay, not a byte-verified historical
    // checkout — see the file-header disclosure. Listed explicitly rather than left implicit.
    undrivable_fixtures: cases.map((entry) => ({
      id: entry.id,
      side: "baseline",
      reason: "baseline is a literal transcription of the fixture's documented failure input; this environment's primary-checkout-only and no-worktree-without-approval rules block re-executing a historical commit to verify it byte-for-byte",
    })),
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = runM0();
  process.stdout.write(`${JSON.stringify(result)}\n`);
  if (!result.passed) process.exitCode = 1;
}
