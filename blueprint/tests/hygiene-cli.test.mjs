import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.resolve(HERE, "../scripts/blueprint.mjs");

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

function findAuditCollector() {
  let current = HERE;
  while (true) {
    const candidate = path.join(current, "tools", "skills", "audit", "collect-facts.mjs");
    if (fs.existsSync(candidate)) return candidate;
    const parent = path.dirname(current);
    if (parent === current) return null;
    current = parent;
  }
}

test("Blueprint hygiene facts are cached, generation-bound, and become stale with the graph", {
  skip: findAuditCollector() ? false : "requires workspace Audit collector",
}, () => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "blueprint-hygiene-"));
  try {
    fs.mkdirSync(path.join(repo, "src"));
    fs.writeFileSync(path.join(repo, "src", "large.ts"), [
      "export function alpha() {",
      ...Array.from({ length: 207 }, (_, index) => `  // alpha ${index + 1}`),
      "}",
      "export function beta() {",
      ...Array.from({ length: 207 }, (_, index) => `  // beta ${index + 1}`),
      "}",
      "",
    ].join("\n"));
    // Above-trigger file in a language the graph provider does NOT parse — its candidate must say
    // graphMetrics "unavailable", never fabricated zero symbol counts.
    fs.writeFileSync(path.join(repo, "src", "service.go"), [
      "package service",
      ...Array.from({ length: 500 }, (_, index) => `// go line ${index + 1}`),
      "",
    ].join("\n"));
    assert.equal(spawnSync("git", ["init", "--quiet"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["add", "src/large.ts", "src/service.go"], { cwd: repo }).status, 0);

    const graphBuild = run(repo, ["graph", "build", "--out", ".agent"]);
    assert.equal(graphBuild.status, 0, graphBuild.stderr || graphBuild.stdout);
    const configPath = path.join(repo, ".agent", "config.json");
    fs.writeFileSync(configPath, `${JSON.stringify({ hygiene: { decompositionReviewLoc: 410 } }, null, 2)}\n`);

    const missing = run(repo, ["hygiene", "status", "--out", ".agent", "--json"]);
    assert.equal(missing.status, 2);
    assert.equal(JSON.parse(missing.stdout).state, "missing");

    const refresh = run(repo, [
      "hygiene", "refresh", "--out", ".agent", "--only", "decomposition,debt_markers", "--json",
    ]);
    assert.equal(refresh.status, 0, refresh.stderr || refresh.stdout);
    const refreshed = JSON.parse(refresh.stdout);
    assert.equal(refreshed.state, "fresh");
    assert.deepEqual(refreshed.selectedChecks, ["decomposition", "debt_markers"]);
    assert.match(refreshed.sourceGenerationId, /^xxh128:[a-f0-9]{32}$/);
    assert.ok(fs.existsSync(path.join(repo, ".agent", "hygiene", "facts.json")));
    const decomposition = refreshed.checks.find((check) => check.check === "decomposition");
    assert.equal(decomposition.findings_count, 0);
    assert.equal(decomposition.candidate_count, 2);
    assert.equal(decomposition.meta.threshold, 410);
    assert.equal(decomposition.meta.thresholdKind, "review-trigger");
    assert.equal(decomposition.meta.graphMetricsVersion, 2);
    const parsedCandidate = decomposition.meta.oversized.find((item) => item.file === "src/large.ts");
    assert.equal(parsedCandidate.graphMetrics, "available");
    assert.equal(parsedCandidate.symbolCount, 2);
    assert.ok(parsedCandidate.bytes > 0);
    assert.ok(parsedCandidate.largestSymbolLines >= 200);
    const unparsedCandidate = decomposition.meta.oversized.find((item) => item.file === "src/service.go");
    assert.equal(unparsedCandidate.graphMetrics, "unavailable");
    assert.match(unparsedCandidate.graphMetricsReason, /language not parsed/);
    assert.ok(!("symbolCount" in unparsedCandidate));

    const fresh = run(repo, ["hygiene", "status", "--out", ".agent", "--json"]);
    assert.equal(fresh.status, 0, fresh.stderr || fresh.stdout);
    assert.equal(JSON.parse(fresh.stdout).state, "fresh");

    const offline = run(repo, [
      "hygiene", "refresh", "--out", ".agent", "--only", "outdated,cargo_outdated", "--offline", "--json",
    ]);
    assert.equal(offline.status, 0, offline.stderr || offline.stdout);
    assert.ok(JSON.parse(offline.stdout).checks.every((check) => check.status === "skipped" && check.skip_reason === "offline mode"));

    fs.appendFileSync(path.join(repo, "src", "large.ts"), "// changed\n");
    const stale = run(repo, ["hygiene", "status", "--out", ".agent", "--json"]);
    assert.equal(stale.status, 2);
    assert.equal(JSON.parse(stale.stdout).state, "stale");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("standalone hygiene reports a missing Audit collector as unavailable", () => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "blueprint-hygiene-standalone-"));
  try {
    fs.mkdirSync(path.join(repo, "src"));
    fs.writeFileSync(path.join(repo, "src", "index.ts"), "export const value = 1;\n");
    assert.equal(run(repo, ["graph", "build", "--out", ".agent"]).status, 0);
    const result = spawnSync(process.execPath, [CLI, "hygiene", "refresh", "--out", ".agent", "--json"], {
      cwd: repo,
      encoding: "utf8",
      env: { ...process.env, BLUEPRINT_AUDIT_FACT_COLLECTOR: path.join(repo, "missing-collector.mjs") },
    });
    assert.equal(result.status, 2, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.state, "unavailable");
    assert.equal(payload.reasonCode, "audit_collector_missing");
  } finally { fs.rmSync(repo, { recursive: true, force: true }); }
});
