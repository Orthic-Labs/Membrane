import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

test("Cortex CLI exposes the orient contract and branded help", () => {
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  assert.equal(pkg.bin.cortex, "./scripts/cortex.mjs");
  assert.equal(pkg.bin["cortex-watch"], "./scripts/cortex-watch.mjs");
  assert.equal(pkg.bin["cortex-mcp"], "./scripts/cortex-mcp.mjs");
  assert.equal(pkg.bin["cortex-install"], "./scripts/cortex-install.mjs");
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0);
  assert.match(help.stdout, /Cortex — repository truth and evidence map/);

  const repo = mkdtempSync(join(tmpdir(), "cortex-alias-"));
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const orient = spawnSync(process.execPath, [CLI, "orient", "--json", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(orient.status, 0, orient.stderr || orient.stdout);
    const payload = JSON.parse(orient.stdout);
    assert.deepEqual(Object.keys(payload).sort(), ["candidates", "entrypoint", "freshness", "freshnessReceipt", "generationId", "manifestDigest", "product", "schemaVersion", "topCircuits"].sort());
    assert.equal(payload.schemaVersion, 1);
    assert.equal(payload.product, "cortex");
    assert.equal(typeof payload.freshness.state, "string");
    assert.ok(payload.candidates);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
