#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";

const membraneRoot = resolve(dirname(new URL(import.meta.url).pathname), "..");
const root = resolve(membraneRoot, "..");
const python = join(root, ".venv-tools", "bin", "python");
const candidates = [
  join(root, "tools", "bin", process.platform === "win32" ? "cortex.exe" : "cortex"),
  join(membraneRoot, "engine", "target", "debug", "cortex"),
];
const cortex = candidates.find(existsSync);
const sourceRunner = join(membraneRoot, "adapt", "src", "adapt", "run_incremental_multiwriter.py");
const dailySync = join(root, "tools", "pipelines", "memory", "daily-sync.sh");
const installedShim = join(homedir(), "bin", process.platform === "win32" ? "adapt.cmd" : "adapt");
const sourceCli = join(membraneRoot, "adapt", "src", "adapt", "cli.py");
const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

if (!cortex) throw new Error("current Cortex binary is unavailable");
for (const path of [python, sourceRunner, dailySync, installedShim, sourceCli]) {
  if (!existsSync(path)) throw new Error(`required Adapt authority path missing: ${path}`);
}
const scheduleText = readFileSync(dailySync, "utf8");
const shimText = readFileSync(installedShim, "utf8");
const schedulerCurrent = scheduleText.includes(
  'adapt_runner="$SOURCE_WS/membrane/adapt/src/adapt/run_incremental_multiwriter.py"',
);
const shimCurrent = shimText.includes(sourceCli);
const execution = spawnSync(
  python,
  ["-m", "pytest", join(membraneRoot, "adapt", "tests", "test_adapt_event_learning.py"), "-q"],
  {
    cwd: root,
    encoding: "utf8",
    timeout: 180_000,
    env: {
      ...process.env,
      ADAPT_E2E: "1",
      CORTEX_BIN: cortex,
      CORTEX_LIVE_DB: process.env.CORTEX_DB || join(root, "tools", ".cache", "memory", "cortex-engine.db"),
    },
  },
);
const result = {
  schema: "membrane.adapt-installed-current.v1",
  passed: execution.status === 0 && schedulerCurrent && shimCurrent,
  source: {
    runner: sourceRunner,
    runner_sha256: sha256(sourceRunner),
    cli: sourceCli,
    cli_sha256: sha256(sourceCli),
  },
  installed: {
    scheduler_runner: sourceRunner,
    scheduler_runner_sha256: sha256(sourceRunner),
    shim: installedShim,
    shim_sha256: sha256(installedShim),
    scheduler_current: schedulerCurrent,
    shim_current: shimCurrent,
  },
  e2e: {
    status: execution.status,
    output_sha256: createHash("sha256").update(`${execution.stdout || ""}${execution.stderr || ""}`).digest("hex"),
    output_tail: `${execution.stdout || ""}${execution.stderr || ""}`.slice(-2000),
  },
};
process.stdout.write(`${JSON.stringify(result)}\n`);
if (!result.passed) process.exitCode = 1;
