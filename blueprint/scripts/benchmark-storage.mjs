#!/usr/bin/env node
// Step 1 storage/incremental baseline harness.  Every fixture is disposable;
// reports contain raw observations so later changes are compared to evidence.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import { join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { generateBenchmarkFixture, fixtureFilePath } from "./generate-benchmark-fixture.mjs";
import { openStore, openStoreReadOnly, closeStore, searchGenerationSymbols, getGenerationEnvelope, listFileMetadata } from "../src/graph/store-sqlite.mjs";
import { applyFileDelta, STRUCTURAL_PROVIDER } from "../src/graph/delta-store.mjs";
import { parseFileFacts } from "../src/graph/static-provider.mjs";
import { stableRead } from "../src/graph/stable-read.mjs";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const CLI = join(ROOT, "scripts", "blueprint.mjs");
export const REPORT_SCHEMA = "blueprint-storage-benchmark-report-v1";

function p95(values) {
  if (!values.length) return null;
  return [...values].sort((a, b) => a - b)[Math.max(0, Math.ceil(values.length * 0.95) - 1)];
}
function bytes(path) { return existsSync(path) ? statSync(path).size : 0; }
function maxRssBytes(rssKb) { return Number(rssKb ?? 0) * 1024; }
function gitCommit() { try { return execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(); } catch { return null; } }
function gitWorktreeState() { try { return execFileSync("git", ["status", "--porcelain=v1"], { cwd: ROOT, encoding: "utf8" }).trim() ? "dirty" : "clean"; } catch { return "unknown"; } }
function sqliteVersion(db) { return db.prepare("SELECT sqlite_version() AS version").get().version; }

async function runBuild(repo) {
  const started = performance.now();
  const child = spawn(process.execPath, [CLI, "build", "--out", ".agent"], { cwd: repo, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "", stderr = "", peakRssBytes = 0;
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const sampleRss = () => {
    try {
      const value = execFileSync("ps", ["-o", "rss=", "-p", String(child.pid)], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
      peakRssBytes = Math.max(peakRssBytes, maxRssBytes(Number(value)));
    } catch { /* process may already have exited */ }
  };
  sampleRss();
  const poll = setInterval(sampleRss, 10);
  const result = await new Promise((resolveResult, reject) => child.once("error", reject).once("close", (code, signal) => resolveResult({ code, signal })));
  clearInterval(poll);
  sampleRss();
  if (result.code !== 0) throw new Error(`build failed (${result.code ?? result.signal}): ${stderr.slice(-1200)}`);
  if (peakRssBytes === 0) throw new Error("build RSS sampling produced no observation");
  return { elapsedMs: performance.now() - started, peakRssBytes, stdout: stdout.trim(), rssProbe: "ps -o rss= -p <pid> (10ms interval)" };
}

async function applyDelta(repo, index, revision) {
  const relative = fixtureFilePath(index);
  const path = join(repo, relative);
  writeFileSync(path, `${readFileSync(path, "utf8")}\nexport const benchmarkRevision${revision} = ${revision};\n`);
  const read = stableRead(path);
  const db = openStore(join(repo, ".agent", "graph", "graph.db"));
  try {
    const descriptor = { absolutePath: path, path: relative, text: read.bytes.toString("utf8"), lines: read.bytes.toString("utf8").split(/\r?\n/), contentHash: read.contentDigest.replace(/^xxh128:/, ""), size: read.bytes.length };
    const files = listFileMetadata(db).filter((file) => file.path !== relative).concat(descriptor);
    const parsed = parseFileFacts(repo, descriptor, { files });
    const started = performance.now();
    const result = applyFileDelta(db, { eventKind: "modify", path: relative, parsed, provider: STRUCTURAL_PROVIDER, contentDigest: read.contentDigest, size: read.bytes.length, mtimeMs: read.statAfter.mtimeMs });
    return { elapsedMs: performance.now() - started, applied: result.applied === true, peakRssBytes: process.memoryUsage().rss };
  } finally { closeStore(db); }
}

function allocation(db) {
  try {
    const rows = db.prepare("SELECT name, SUM(pgsize) AS bytes, COUNT(*) AS pages FROM dbstat GROUP BY name ORDER BY bytes DESC").all();
    return { state: "available", rows: rows.map((row) => ({ name: row.name, bytes: Number(row.bytes), pages: Number(row.pages) })) };
  } catch (error) { return { state: "unavailable", reason: String(error.message) }; }
}

async function residentLookup({ repo, query, command }) {
  if (!command) return { state: "not_configured", samples: [], p95Ms: null };
  if (typeof command !== "string" || !command.trim()) throw new Error("resident lookup hook must be one executable path");
  const samples = [];
  for (let index = 0; index < 10; index += 1) {
    const started = performance.now();
    const child = spawn(command, [repo, query], { stdio: "ignore" });
    const code = await new Promise((resolveResult, reject) => child.once("error", reject).once("close", resolveResult));
    if (code !== 0) throw new Error(`resident lookup hook exited ${code}`);
    samples.push(performance.now() - started);
  }
  return { state: "measured", command: `${command} [repo] [query]`, samples, p95Ms: p95(samples) };
}

export function validateStorageReport(report) {
  const problems = [];
  if (report?.schema !== REPORT_SCHEMA) problems.push("schema");
  if (!report?.source?.commit) problems.push("source.commit");
  if (!report?.source?.worktree) problems.push("source.worktree");
  if (!report?.fixture?.id || !Number.isInteger(report?.fixture?.files)) problems.push("fixture");
  if (!report?.host?.node || !report?.host?.platform) problems.push("host");
  for (const name of ["coldBuild", "noChange", "oneFileDelta"]) {
    const metric = report?.metrics?.[name];
    if (!Array.isArray(metric?.samples) || metric.samples.length < 1) problems.push(`${name}.samples`);
    if (!Number.isFinite(metric?.peakRssBytes) || metric.peakRssBytes <= 0) problems.push(`${name}.peakRssBytes`);
  }
  if (!report?.metrics?.walBlockedReader || !report?.metrics?.allocation) problems.push("WAL/allocation");
  return { ok: problems.length === 0, problems };
}

export async function runStorageBenchmark({ fixtureFiles = 550, samples = 5, reportPath = null, fixtureDir = null, keepFixture = false, residentLookupCommand = null } = {}) {
  if (!Number.isInteger(samples) || samples < 1) throw new Error("samples must be a positive integer");
  const repo = fixtureDir ? resolve(fixtureDir) : mkdtempSync(join(os.tmpdir(), "blueprint-storage-bench-"));
  let generated = false;
  try {
    const fixture = generateBenchmarkFixture({ outDir: repo, files: fixtureFiles, force: Boolean(fixtureDir) });
    generated = true;
    const coldSamples = [];
    for (let index = 0; index < samples; index += 1) {
      if (index > 0) generateBenchmarkFixture({ outDir: repo, files: fixtureFiles, force: true });
      coldSamples.push(await runBuild(repo));
    }
    const noChangeSamples = [];
    for (let index = 0; index < samples; index += 1) noChangeSamples.push(await runBuild(repo));
    const deltaSamples = [];
    for (let index = 0; index < samples; index += 1) deltaSamples.push(await applyDelta(repo, index, index + 1));
    const fixedUpdateStarted = performance.now();
    const fixedUpdates = [];
    for (let index = 0; index < Math.min(100, fixtureFiles); index += 1) fixedUpdates.push(await applyDelta(repo, index, 1000 + index));
    const dbPath = join(repo, ".agent", "graph", "graph.db");
    const writer = openStore(dbPath);
    let walBlockedReader;
    try {
      writer.exec("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA wal_autocheckpoint=0;");
      const reader = openStoreReadOnly(dbPath);
      try {
        reader.exec("BEGIN;");
        reader.prepare("SELECT COUNT(*) FROM symbols").get();
        const beforeBytes = bytes(`${dbPath}-wal`);
        const write = await applyDelta(repo, Math.min(101, fixtureFiles - 1), 99999);
        const afterBytes = bytes(`${dbPath}-wal`);
        const checkpoint = writer.prepare("PRAGMA wal_checkpoint(PASSIVE)").get();
        walBlockedReader = { state: afterBytes > beforeBytes ? "growth_observed" : "no_growth_observed", beforeBytes, afterBytes, growthBytes: afterBytes - beforeBytes, checkpoint: { busy: Number(checkpoint.busy), log: Number(checkpoint.log), checkpointed: Number(checkpoint.checkpointed) }, write };
        reader.exec("COMMIT;");
      } finally { closeStore(reader); }
      writer.exec("PRAGMA wal_checkpoint(TRUNCATE);");
      const envelope = getGenerationEnvelope(writer);
      const lookup = await residentLookup({ repo, query: "benchmarkSymbol00001", command: residentLookupCommand });
      if (lookup.state === "not_configured") {
        const queryStarted = performance.now();
        const rows = searchGenerationSymbols(writer, envelope?.manifest?.generationId, ["benchmarkSymbol00001"]);
        lookup.embeddedControl = { elapsedMs: performance.now() - queryStarted, rows: rows.length, label: "not a resident-daemon measurement" };
      }
      const report = {
        schema: REPORT_SCHEMA,
        command: `node scripts/benchmark-storage.mjs --fixture-files ${fixtureFiles} --samples ${samples}`,
        source: { commit: gitCommit(), root: ROOT, worktree: gitWorktreeState() },
        fixture,
        host: { hostname: os.hostname(), platform: process.platform, arch: process.arch, release: os.release(), node: process.version, sqlite: sqliteVersion(writer) },
        samples,
        metrics: {
          coldBuild: { samples: coldSamples, p95Ms: p95(coldSamples.map((item) => item.elapsedMs)), peakRssBytes: Math.max(...coldSamples.map((item) => item.peakRssBytes)) },
          noChange: { samples: noChangeSamples, p95Ms: p95(noChangeSamples.map((item) => item.elapsedMs)), peakRssBytes: Math.max(...noChangeSamples.map((item) => item.peakRssBytes)) },
          oneFileDelta: { samples: deltaSamples, p95Ms: p95(deltaSamples.map((item) => item.elapsedMs)), peakRssBytes: Math.max(...deltaSamples.map((item) => item.peakRssBytes)) },
          residentExactLookup: lookup,
          fixed100FileUpdate: { files: fixedUpdates.length, elapsedMs: performance.now() - fixedUpdateStarted, samples: fixedUpdates },
          walBlockedReader,
          allocation: allocation(writer),
          databaseBytes: bytes(dbPath),
          walBytesAfterQuiescentCheckpoint: bytes(`${dbPath}-wal`),
        },
      };
      const checked = validateStorageReport(report);
      if (!checked.ok) throw new Error(`invalid benchmark report: ${checked.problems.join(", ")}`);
      if (reportPath) writeFileSync(resolve(reportPath), `${JSON.stringify(report, null, 2)}\n`);
      return report;
    } finally { closeStore(writer); }
  } finally {
    if (generated && !keepFixture && !fixtureDir) rmSync(repo, { recursive: true, force: true });
  }
}

function parseArgs(argv) {
  const result = { fixtureFiles: 550, samples: 5, reportPath: null, fixtureDir: null, keepFixture: false, residentLookupCommand: process.env.BLUEPRINT_RESIDENT_LOOKUP_HOOK ?? null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--fixture-files") result.fixtureFiles = Number(argv[++index]);
    else if (arg === "--samples") result.samples = Number(argv[++index]);
    else if (arg === "--report") result.reportPath = argv[++index];
    else if (arg === "--fixture-dir") result.fixtureDir = argv[++index];
    else if (arg === "--keep-fixture") result.keepFixture = true;
    else if (arg === "--resident-lookup-command") result.residentLookupCommand = argv[++index];
    else throw new Error(`unknown argument: ${arg}`);
  }
  return result;
}

const invoked = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) process.stdout.write(`${JSON.stringify(await runStorageBenchmark(parseArgs(process.argv.slice(2))), null, 2)}\n`);
