import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

test("doctor reports PR11 operational checks and ordinary deltas do not rebuild", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-qualification-"));
  try {
    cpSync(FIXTURE, repo, { recursive: true });
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    const first = run(repo, ["doctor", "--json", "--out", ".agent"]);
    assert.equal(first.status, 0, first.stderr);
    const firstPayload = JSON.parse(first.stdout);
    assert.equal(firstPayload.telemetry.full_rebuilds, 1);
    assert.equal(firstPayload.telemetry.deltas_applied, 0);
    const { mcpHandshake } = firstPayload.operational;
    assert.equal(mcpHandshake.ready, true, JSON.stringify({ timedOut: mcpHandshake.timedOut, exitCode: mcpHandshake.exitCode }));
    assert.equal(firstPayload.operational.receiptTable, true);
    assert.ok(firstPayload.reasons.some((reason) => reason.code === "snapshot_presence"));
    assert.ok(firstPayload.reasons.some((reason) => reason.code === "grant_key_presence"));

    const source = join(repo, "src/service.ts");
    writeFileSync(source, `${readFileSync(source, "utf8")}\nexport const qualificationDelta = true;\n`);
    const delta = run(repo, ["delta", "src/service.ts", "--out", ".agent"]);
    assert.equal(delta.status, 0, delta.stderr);
    const second = run(repo, ["doctor", "--json", "--out", ".agent"]);
    assert.equal(second.status, 0, second.stderr);
    const secondPayload = JSON.parse(second.stdout);
    assert.equal(secondPayload.telemetry.full_rebuilds, 1);
    assert.equal(secondPayload.telemetry.deltas_applied, 1);
    assert.equal(secondPayload.telemetry.stale_results, 0);
    assert.ok(secondPayload.telemetry.barrier_p95_ms >= 0);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
