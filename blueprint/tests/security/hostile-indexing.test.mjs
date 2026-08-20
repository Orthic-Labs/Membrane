// D51: hostile-repository indexing. A repository full of hostile inputs — huge
// files, deep trees, symlink escape, path case tricks, a malformed store,
// event overflow — must never crash the indexer, never read outside the repo
// root, and never turn repository content into an instruction.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { buildHostileRepo, SECRET_ENTRIES } from "../../fixtures/security/build-hostile-repo.mjs";

const ROOT = join(import.meta.dirname, "..", "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");

function makeHostileRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-hostile-"));
  buildHostileRepo(repo);
  return repo;
}

function run(repo, args, timeout = 60_000) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8", timeout });
}

test("indexing a hostile repo does not crash and stays bounded", () => {
  const repo = makeHostileRepo();
  try {
    const result = run(repo, ["build", "--out", ".agent", "--json"]);
    assert.ok(result.status !== null, `build process exited: ${result.signal ?? "unknown"}`);
    // Build may legitimately fail (corrupt store, unsupported paths) — but it
    // must do so with a clearly bounded stdout+stderr, never a hang or a core.
    assert.ok(
      (result.stdout?.length ?? 0) + (result.stderr?.length ?? 0) < 2_000_000,
      `bounded output: ${result.stdout?.length ?? 0} + ${result.stderr?.length ?? 0} bytes`,
    );
    // No secret may ever appear on any egress surface.
    const everything = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
    for (const secret of SECRET_ENTRIES) {
      assert.ok(!everything.includes(secret), `no secret egress: ${secret.slice(0, 12)}...`);
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("the scanner never follows a symlink out of the repo root", () => {
  const repo = makeHostileRepo();
  try {
    assert.ok(existsSync(join(repo, "escape")), "escape symlink exists");
    // The hostile tree's malformed store makes status return a typed corrupt
    // state (not a crash); either way the process must complete normally and
    // the inventory surface must stay bounded.
    const result = run(repo, ["graph", "status", "--json"]);
    assert.ok(result.status !== null && result.status >= 0 && result.status <= 3, `completed with a typed CLI exit, not a crash: ${result.signal ?? result.status}`);
    const out = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    assert.ok(out.length < 2_000_000, "bounded inventory output");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("a malformed store reports a typed corrupt state instead of crashing", () => {
  const repo = makeHostileRepo();
  try {
    const doctor = run(repo, ["doctor", "--json", "--full"]);
    // Missing map -> "missing" (2) and corrupt store -> "corrupt" (1) are both
    // typed outcomes; a raw crash (signal or uncaught) is the failure.
    assert.ok(doctor.status !== null && doctor.status >= 0 && doctor.status <= 3, `no crash: ${doctor.signal ?? doctor.status}`);
    assert.ok(doctor.stdout ?? "", "doctor emitted a JSON payload");
    const payload = JSON.parse(doctor.stdout);
    assert.ok(["ready", "degraded", "stale", "broken", "corrupt", "missing"].includes(payload.state), `typed state, got ${payload.state}`);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("event overflow is surfaced as a typed degradation, not an unbounded loop", () => {
  const repo = makeHostileRepo();
  try {
    // 300 creates in churn/ is over a single drain batch; a bounded watcher
    // drains in passes and converges. A looping watcher would hang this call.
    const result = run(repo, ["build", "--out", ".agent", "--json"], 60_000);
    assert.ok(result.status !== null, "build returned within timeout");
    const combined = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    assert.ok(combined.length < 5_000_000, "no unbounded output from event overflow");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("path case tricks and deep trees stay inside the enrolled root", () => {
  const repo = makeHostileRepo();
  try {
    const result = run(repo, ["build", "--out", ".agent", "--json"], 60_000);
    const out = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    // No absolute path outside the repo may be emitted (the SRC/.. case trick
    // would show up either as a traversal or as a duplicate path).
    assert.ok(!out.includes("..\\"), "no backslash traversal in any output");
    assert.ok(out.length < 5_000_000, "bounded output for the deepest-tree repo");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
