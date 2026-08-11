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
  join(root, "tools", "bin", process.platform === "win32" ? "crypt.exe" : "crypt"),
  join(membraneRoot, "engine", "target", "debug", "crypt"),
];
const crypt = candidates.find(existsSync);
const sourceRunner = join(root, "adapt", "run_incremental_multiwriter.py");
const staleRunner = join(root, "tools", "pipelines", "memory", "adapt", "run_incremental_multiwriter.py");
const dailySync = join(root, "tools", "pipelines", "memory", "daily-sync.sh");
const installedShim = join(homedir(), "bin", process.platform === "win32" ? "adapt.cmd" : "adapt");
const sourceCli = join(root, "adapt", "cli.py");
const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

if (!crypt) throw new Error("current Crypt binary is unavailable");
for (const path of [python, sourceRunner, dailySync, installedShim, sourceCli]) {
  if (!existsSync(path)) throw new Error(`required Adapt authority path missing: ${path}`);
}
const scheduleText = readFileSync(dailySync, "utf8");
const shimText = readFileSync(installedShim, "utf8");
const schedulerCurrent = scheduleText.includes('adapt_runner="$SOURCE_WS/adapt/run_incremental_multiwriter.py"')
  && !scheduleText.includes('adapt_runner="$SOURCE_WS/tools/pipelines/memory/adapt/run_incremental_multiwriter.py"');
const shimCurrent = shimText.includes(sourceCli);
const execution = spawnSync(
  python,
  ["-m", "pytest", join(root, "adapt", "test_adapt_event_learning.py"), "-q"],
  {
    cwd: root,
    encoding: "utf8",
    timeout: 180_000,
    env: {
      ...process.env,
      ADAPT_E2E: "1",
      CRYPT_BIN: crypt,
      CRYPT_LIVE_DB: process.env.CRYPT_DB || join(root, "tools", ".cache", "memory", "crypt-engine.db"),
    },
  },
);
const result = {
  schema: "orthic.adapt-installed-current.v1",
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
  stale_copy: existsSync(staleRunner) ? { path: staleRunner, sha256: sha256(staleRunner), active: false } : null,
  e2e: {
    status: execution.status,
    output_sha256: createHash("sha256").update(`${execution.stdout || ""}${execution.stderr || ""}`).digest("hex"),
    output_tail: `${execution.stdout || ""}${execution.stderr || ""}`.slice(-2000),
  },
};
process.stdout.write(`${JSON.stringify(result)}\n`);
if (!result.passed) process.exitCode = 1;
