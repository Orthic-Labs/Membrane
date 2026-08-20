import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

test("Blueprint CLI exposes the recall contract and branded help", () => {
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  assert.equal(pkg.bin.blueprint, "./scripts/blueprint.mjs");
  assert.equal(pkg.bin["blueprint-watch"], "./scripts/blueprint-watch.mjs");
  assert.equal(pkg.bin["blueprint-mcp"], "./scripts/blueprint-mcp.mjs");
  assert.equal(pkg.bin["blueprint-install"], "./scripts/blueprint-install.mjs");
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0);
  assert.match(help.stdout, /Blueprint — repository truth and evidence map/);

  const repo = mkdtempSync(join(tmpdir(), "blueprint-alias-"));
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const recall = spawnSync(process.execPath, [CLI, "recall", "--json", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(recall.status, 0, recall.stderr || recall.stdout);
    const payload = JSON.parse(recall.stdout);
    assert.deepEqual(Object.keys(payload).sort(), ["candidates", "entrypoint", "freshness", "freshnessReceipt", "generationId", "manifestDigest", "product", "schemaVersion", "topCircuits"].sort());
    assert.equal(payload.schemaVersion, 1);
    assert.equal(payload.product, "blueprint");
    assert.equal(typeof payload.freshness.state, "string");
    assert.ok(payload.candidates);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
