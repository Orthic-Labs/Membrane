#!/usr/bin/env node
// scripts/qualification/run-adversarial.mjs — MBR-802 qualification entrypoint.
//
// Executes the authorization + prompt-injection adversarial suite via the real
// Node test runner, then independently replays every "deny" fixture case to
// compute the unauthorized admission rate. Exits non-zero on ANY unauthorized
// admission. The release/book gate invokes:
//
//     node scripts/qualification/run-adversarial.mjs --all
//
// and requires a zero unauthorized-admission rate.

import { spawnSync } from "node:child_process";
import { pathToFileURL, fileURLToPath } from "node:url";
import { dirname, resolve, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "../..");
const TEST_GLOB = join(ROOT, "tests", "adversarial", "*.test.mjs");
const AUTH_URL = pathToFileURL(resolve(ROOT, "mcp/authorization.mjs")).href;

const args = process.argv.slice(2);
const RUN_ALL = args.includes("--all");
const VERBOSE = args.includes("--verbose");

if (!RUN_ALL && args.length > 0 && !VERBOSE) {
  console.error(`unknown argument(s): ${args.filter((a) => a !== "--verbose").join(" ")}`);
  console.error("usage: node scripts/qualification/run-adversarial.mjs [--all] [--verbose]");
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Lane 1: run the adversarial test files through the Node test runner.
// ---------------------------------------------------------------------------
function runTestSuite() {
  const result = spawnSync(process.execPath, ["--test", TEST_GLOB], {
    cwd: ROOT, encoding: "utf8",
  });
  return { status: result.status ?? 1, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

// ---------------------------------------------------------------------------
// Lane 2: independent admission-rate computation over the deny battery.
// ---------------------------------------------------------------------------
async function computeAdmissionRate() {
  const { buildCases } = await import(pathToFileURL(resolve(ROOT, "tests/fixtures/adversarial/fixtures.mjs")).href);
  const { authorizeTarget } = await import(AUTH_URL);
  const cases = buildCases().filter((c) => c.expect === "deny");
  let attempted = 0;
  let admitted = 0;
  const admittedIds = [];
  for (const c of cases) {
    attempted += 1;
    try {
      await authorizeTarget({
        callerBinding: c.caller,
        targetBinding: c.target,
        childGrantLevel: c.childGrantLevel,
        taskGrantLevel: c.taskGrantLevel,
        installationLevel: c.installationLevel ?? "write-proposed",
        action: c.action,
        hasExplicitChildGrant: c.hasExplicitChildGrant,
      });
      admitted += 1;
      admittedIds.push(c.id);
    } catch {
      // denied — expected
    }
  }
  const rate = attempted === 0 ? 0 : admitted / attempted;
  return { attempted, admitted, admittedIds, rate };
}

const suite = runTestSuite();
if (VERBOSE) process.stdout.write(suite.output);
const admission = await computeAdmissionRate();

const suitePassed = suite.status === 0;
const zeroAdmission = admission.admitted === 0;
const passed = suitePassed && zeroAdmission;

const report = {
  task: "MBR-802",
  gate: "adversarial-qualification",
  suite: { exit_status: suite.status, passed: suitePassed },
  unauthorized_admission: {
    attempted: admission.attempted,
    admitted: admission.admitted,
    rate: admission.rate,
    admitted_ids: admission.admittedIds,
  },
  zero_unauthorized_admission: zeroAdmission,
  passed,
};

console.log(JSON.stringify(report, null, 2));

if (!passed) {
  if (!suitePassed) {
    process.stderr.write("\n[adversarial] test suite FAILED — full output follows\n");
    process.stderr.write(suite.output);
  }
  if (!zeroAdmission) {
    process.stderr.write(
      `\n[adversarial] UNAUTHORIZED ADMISSIONS: ${admission.admittedIds.join(", ")}\n`,
    );
  }
  process.exit(1);
}

console.log(
  `\n[adversarial] PASS — unauthorized admission rate ${admission.rate} (${admission.admitted}/${admission.attempted}).`,
);
process.exit(0);
