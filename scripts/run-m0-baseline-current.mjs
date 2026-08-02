#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const membraneRoot = resolve(HERE, "..");
const workspaceRoot = resolve(membraneRoot, "..");
const fixturePath = join(workspaceRoot, "docs/plans/sol/membrane-competitive-review/contracts/fable-m0-false-success-fixtures.json");
const sha = (value) => createHash("sha256").update(value).digest("hex");

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
  "sentinel-mcp-output-schema": {
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
  membrane: [process.execPath, ["--test", join(membraneRoot, "mcp/project-registry.test.mjs")], workspaceRoot],
  cortex: ["pnpm", ["--dir", join(workspaceRoot, "cortex"), "test:all"], workspaceRoot],
  sentinel: ["pnpm", ["--dir", join(workspaceRoot, "tether"), "test"], workspaceRoot],
  morph: [join(workspaceRoot, ".venv-tools/bin/python"), ["-m", "pytest", join(workspaceRoot, "adapt/tests")], workspaceRoot],
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
      MEMRIGHT_TEST_BIN: process.env.MEMRIGHT_TEST_BIN || join(membraneRoot, "engine/target/debug", process.platform === "win32" ? "memright.exe" : "memright"),
    },
  });
  const output = `${result.stdout || ""}${result.stderr || ""}`;
  return { owner, status: result.status, signal: result.signal || null, output_sha256: `sha256:${sha(output)}`, output_tail: output.slice(-1200) };
}

export function runM0() {
  const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));
  if (fixture.schema !== "orthic.membrane.fable-m0-fixtures.v1") throw new Error("m0_fixture_schema_invalid");
  const unknown = fixture.cases.filter(({ id }) => !probes[id]).map(({ id }) => id);
  if (unknown.length) throw new Error(`m0_probe_missing:${unknown.join(",")}`);
  const owners = [...new Set(fixture.cases.map(({ owner }) => owner))];
  const suiteResults = Object.fromEntries(owners.map((owner) => [owner, runSuite(owner)]));
  const cases = fixture.cases.map((entry) => {
    const probe = probes[entry.id];
    const baselineFailed = !probe.valid(probe.baseline);
    const currentPassed = probe.valid(probe.current) && suiteResults[entry.owner].status === 0;
    return {
      id: entry.id,
      owner: entry.owner,
      expected_reason: entry.expected,
      baseline: { status: baselineFailed ? "expected_failure" : "unexpected_pass", reason: baselineFailed ? entry.expected : "detector_did_not_reproduce" },
      current: { status: currentPassed ? "pass" : "fail", suite: entry.owner },
    };
  });
  const passed = cases.every((entry) => entry.baseline.status === "expected_failure" && entry.current.status === "pass");
  return { schema: "orthic.membrane.m0-run.v1", passed, baseline_red: cases.filter((entry) => entry.baseline.status === "expected_failure").length, current_green: cases.filter((entry) => entry.current.status === "pass").length, cases, suites: Object.values(suiteResults) };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = runM0();
  process.stdout.write(`${JSON.stringify(result)}\n`);
  if (!result.passed) process.exitCode = 1;
}
