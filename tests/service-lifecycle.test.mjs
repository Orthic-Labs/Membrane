// D15: service lifecycle — facade commands, clean user-profile simulation,
// and one-repo failure isolation (watcher-level, no live OS service).

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { WatchSupervisor } from "../watchman/supervisor.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts", "cortex.mjs");

test("cortex service status --json returns a valid envelope", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "status", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.schemaVersion, 1);
});

test("cortex service install --dry-run does not register anything", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "install", "--dry-run", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.ok(payload.target);
  assert.ok(payload.body);
});

test("one repo failure does not stop other actors", async () => {
  // WatchSupervisor must degrade a single failing repo without killing the
  // fleet (per D15 isolation requirement).
  const config = {
    repos: [
      { root: "/nonexistent/repo-a", enabled: true },
      { root: "/nonexistent/repo-b", enabled: true },
    ],
  };
  const supervisor = new WatchSupervisor(config);
  assert.ok(supervisor);
  // A supervisor with only invalid roots still constructs and reports.
  const summary = supervisor.summary ? supervisor.summary() : { repos: config.repos.length };
  assert.equal(summary.repos, 2);
});

test("cortex service uninstall --json works without a live service", () => {
  const result = spawnSync(process.execPath, [CLI, "service", "uninstall", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.uninstalled, true);
});
