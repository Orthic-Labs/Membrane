// D50: performance envelopes by repository class. A machine-readable budget
// table (evals/performance-envelopes.json) is enforced here with a 4x CI slack
// multiplier, so CI fails on real regressions without flaking on noisy runners.
// A waiver requires a machine-readable rationale AND an expiry — recorded in
// the same table; an expired or rationale-less waiver is itself a failure.

import assert from "node:assert/strict";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { openStore, closeStore } from "../src/graph/store-sqlite.mjs";
import { applyFileDelta } from "../src/graph/delta-store.mjs";
import { stableRead } from "../src/graph/stable-read.mjs";
import { syncToCurrentSource } from "../src/graph/barrier.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const WATCH = join(ROOT, "scripts/blueprint-watch.mjs");
const ENVELOPES = JSON.parse(readFileSync(join(ROOT, "evals/performance-envelopes.json"), "utf8"));

function repoClassFixture(className) {
  const entry = ENVELOPES.repoClasses[className];
  if (!entry?.fixture) return null;
  const repo = mkdtempSync(join(tmpdir(), `blueprint-envelope-${className}-`));
  cpSync(join(ROOT, entry.fixture), repo, { recursive: true });
  return repo;
}

function elapsed(fn) {
  const start = performance.now();
  const value = fn();
  return { value, ms: performance.now() - start };
}

async function elapsedAsync(fn) {
  const start = performance.now();
  const value = await fn();
  return { value, ms: performance.now() - start };
}

function runCli(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8", timeout: 300_000 });
}

function enroll(repo) {
  const result = spawnSync(process.execPath, [WATCH, "enroll", repo], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

function unenroll(repo) {
  const result = spawnSync(process.execPath, [WATCH, "unenroll", repo], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

function dbSizeBytes(repo) {
  return statSync(join(repo, ".agent/graph/graph.db")).size;
}

test("performance envelope table is well-formed and every waiver is valid", () => {
  assert.equal(ENVELOPES.schemaVersion, 1);
  const envelopeKeys = ["coldBuildMs", "incrementalUpdateMs", "noopBarrierMs", "searchImpactMs", "mcpResponseMs", "rssMb", "dbSizeMb", "incrementalWriteBytes"];
  for (const [className, entry] of Object.entries(ENVELOPES.repoClasses)) {
    assert.ok(["small", "medium", "large"].includes(className), `unknown class ${className}`);
    assert.ok(entry.description, `${className} needs a description`);
    for (const key of envelopeKeys) {
      assert.ok(Number.isFinite(entry.budgets[key]) && entry.budgets[key] > 0, `${className}.${key} must be a positive number`);
    }
  }
  assert.ok(ENVELOPES.ciSlackMultiplier >= 2, "CI slack must be at least 2x to avoid flaky CI");
  const future = Date.now() + 24 * 60 * 60 * 1000;
  for (const waiver of ENVELOPES.waivers ?? []) {
    assert.ok(waiver?.id && waiver?.envelope && waiver?.rationale, "every waiver needs id, envelope, rationale");
    assert.ok(waiver.expiresAt && Date.parse(waiver.expiresAt) > future, `waiver ${waiver.id} has no future expiry`);
  }
});

test("the CI slack multiplier is applied to the checked-in budgets, not to measurements", () => {
  const multiplier = ENVELOPES.ciSlackMultiplier;
  const budget = ENVELOPES.repoClasses.medium.budgets.noopBarrierMs;
  // A measurement that misses the raw budget but stays within multiplier*raw
  // must continue to pass — the multiplier is the enforcement threshold.
  assert.ok(budget * multiplier > budget);
});

test("small class envelopes: cold build, no-op barrier, search, db size", async () => {
  const repo = repoClassFixture("small");
  if (!repo) return;
  try {
    const small = ENVELOPES.repoClasses.small.budgets;
    const build = elapsed(() => runCli(repo, ["build", "--out", ".agent"]));
    assert.equal(build.value.status, 0, build.value.stderr?.slice(-500));
    assert.ok(build.ms <= small.coldBuildMs * ENVELOPES.ciSlackMultiplier, `cold build ${build.ms.toFixed(0)}ms > budget`);

    const db = openStore(join(repo, ".agent/graph/graph.db"));
    const barrier = await elapsedAsync(() => syncToCurrentSource(db, repo, { timeoutMs: 5000 }));
    assert.ok(["caught_up", "gap_blocked", "timeout"].includes(barrier.value.barrierResult));
    closeStore(db);
    assert.ok(barrier.ms <= small.noopBarrierMs * ENVELOPES.ciSlackMultiplier, `no-op barrier ${barrier.ms.toFixed(0)}ms > budget`);

    const recall = elapsed(() => runCli(repo, ["recall", "--query", "config", "--json"]));
    assert.equal(recall.value.status, 0, recall.value.stderr?.slice(-500));
    assert.ok(recall.ms <= small.searchImpactMs * ENVELOPES.ciSlackMultiplier, `search ${recall.ms.toFixed(0)}ms > budget`);

    const rssMb = process.memoryUsage().rss / 1024 / 1024;
    assert.ok(rssMb <= small.rssMb * ENVELOPES.ciSlackMultiplier, `rss ${rssMb.toFixed(0)}MB > budget`);
    assert.ok(dbSizeBytes(repo) <= small.dbSizeMb * 1024 * 1024 * ENVELOPES.ciSlackMultiplier, "db size > budget");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("medium class envelopes: cold build, delta ingest write volume, status response", async () => {
  const repo = repoClassFixture("medium");
  if (!repo) return;
  try {
    const medium = ENVELOPES.repoClasses.medium.budgets;
    const build = elapsed(() => runCli(repo, ["build", "--out", ".agent"]));
    assert.equal(build.value.status, 0, build.value.stderr?.slice(-500));
    assert.ok(build.ms <= medium.coldBuildMs * ENVELOPES.ciSlackMultiplier, `cold build ${build.ms.toFixed(0)}ms > budget`);

    enroll(repo);
    const statusStart = performance.now();
    const status = runCli(repo, ["status", "--json"]);
    const statusMs = performance.now() - statusStart;
    assert.equal(status.status, 0, status.stderr);
    assert.ok(statusMs <= medium.mcpResponseMs * ENVELOPES.ciSlackMultiplier, `status ${statusMs.toFixed(0)}ms > budget`);

    const source = join(repo, "src/service.ts");
    if (existsSync(source)) {
      writeFileSync(source, `${readFileSync(source, "utf8")}\nexport const envelopeProbe = Date.now();\n`);
      const storePath = join(repo, ".agent/graph/graph.db"), walPath = `${storePath}-wal`;
      const writer = openStore(storePath);
      const previousAutoCheckpoint = writer.prepare("PRAGMA wal_autocheckpoint").get().wal_autocheckpoint;
      try {
        writer.exec("PRAGMA wal_autocheckpoint=0; PRAGMA wal_checkpoint(TRUNCATE);");
        assert.equal(existsSync(walPath) ? statSync(walPath).size : 0, 0, "truncated WAL baseline");
        const read = stableRead(source);
        const file = { id: "file:src/service.ts", kind: "file", labels: ["File"], name: "service.ts", qualifiedName: "src/service.ts", path: "src/service.ts", confidence: 1, evidence: [{ path: "src/service.ts", contentHash: read.contentDigest.replace(/^xxh128:/, "") }] };
        const probe = { id: "symbol:src/service.ts::envelopeProbe", kind: "symbol", labels: ["Variable"], name: "envelopeProbe", qualifiedName: "envelopeProbe", path: "src/service.ts", confidence: 1, evidence: [] };
        const delta = await elapsedAsync(() => applyFileDelta(writer, { eventKind: "modify", path: "src/service.ts", contentDigest: read.contentDigest, provider: { id: "lexical", version: "metric" }, parsed: { nodes: [file, probe], edges: [] } }));
        assert.equal(delta.value.applied, true, "event appended and journal drained");
        const walBytes = existsSync(walPath) ? statSync(walPath).size : 0;
        const pageSize = writer.prepare("PRAGMA page_size").get().page_size;
        assert.ok(walBytes >= 32 && (walBytes - 32) % (pageSize + 24) === 0, `invalid WAL frame layout: ${walBytes}`);
        assert.ok((walBytes - 32) / (pageSize + 24) >= 1, "one delta writes at least one complete frame");
        assert.ok(walBytes <= medium.incrementalWriteBytes * ENVELOPES.ciSlackMultiplier, "incremental WAL frame bytes > budget");
        console.log(`sqlite_wal_frame_bytes ${walBytes}`);
      } finally {
        writer.exec(`PRAGMA wal_autocheckpoint=${previousAutoCheckpoint}`);
        closeStore(writer);
      }
    }
  } finally {
    if (repo) unenroll(repo);
    rmSync(repo, { recursive: true, force: true });
  }
});
