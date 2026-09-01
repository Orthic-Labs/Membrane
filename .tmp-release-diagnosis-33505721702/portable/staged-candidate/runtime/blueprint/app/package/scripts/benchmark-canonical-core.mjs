#!/usr/bin/env node
// Canonical-core benchmark: measure Blueprint against its tracked repository core,
// never against a generated fixture or the live checkout's .agent output.

import { createHash } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import os from "node:os";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

export const REPORT_SCHEMA = "blueprint-canonical-core-benchmark-report-v1";
const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const EXCLUDED_PREFIXES = [".agent/", "vendor-research/", "evals/fixture-repos/", "evals/performance-fixtures/", "evals/performance-baselines/", "coverage/", "dist/", "out/", ".cache/"];
// Current static-provider instrumentation may depend on a concurrently-added
// publication helper. Overlay both explicitly; HEAD remains every other input.
const MEASUREMENT_OVERLAY_PATHS = ["src/graph/static-provider.mjs", "graph/publication.mjs", "scripts/benchmark-canonical-core.mjs"];

function sha256(value) { return createHash("sha256").update(value).digest("hex"); }
function p95(values) { return [...values].sort((left, right) => left - right)[Math.max(0, Math.ceil(values.length * 0.95) - 1)] ?? null; }
function git(args, encoding = "utf8") { return execFileSync("git", args, { cwd: ROOT, encoding, stdio: ["ignore", "pipe", "ignore"] }); }
function worktreeIdentity() {
  const porcelain = git(["status", "--porcelain=v1", "-z", "--untracked-files=all"], "buffer");
  return { state: porcelain.length ? "dirty" : "clean", statusSha256: sha256(porcelain), changedEntries: porcelain.length ? porcelain.toString("utf8").split("\0").filter(Boolean).length : 0 };
}
function toolVersion(command, args = ["--version"]) { try { return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim(); } catch { return null; } }

export function isCanonicalCorePath(path) {
  return !EXCLUDED_PREFIXES.some((prefix) => path === prefix.slice(0, -1) || path.startsWith(prefix));
}

export function selectCanonicalCoreFiles(paths) {
  return [...paths].filter(isCanonicalCorePath).sort();
}

function trackedCoreInventory() {
  const tracked = git(["ls-files", "--cached", "-z"], "buffer").toString("utf8").split("\0").filter(Boolean);
  const files = selectCanonicalCoreFiles(tracked);
  const contentHash = createHash("sha256");
  for (const path of files) {
    contentHash.update(path); contentHash.update("\0"); contentHash.update(execFileSync("git", ["show", `HEAD:${path}`], { cwd: ROOT })); contentHash.update("\0");
  }
  return { files, trackedFileCount: tracked.length, excludedFileCount: tracked.length - files.length, selectionSha256: sha256(files.join("\0")), trackedContentSha256: contentHash.digest("hex") };
}

function measurementContentSha256(snapshot, paths) {
  const hash = createHash("sha256");
  for (const path of paths.sort()) { hash.update(path); hash.update("\0"); hash.update(readFileSync(join(snapshot, path))); hash.update("\0"); }
  return hash.digest("hex");
}

function copyTrackedCore(inventory) {
  const started = performance.now();
  const snapshot = mkdtempSync(join(os.tmpdir(), "blueprint-canonical-core-"));
  for (const path of inventory.files) {
    const target = join(snapshot, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, execFileSync("git", ["show", `HEAD:${path}`], { cwd: ROOT }));
  }
  const overlayPaths = MEASUREMENT_OVERLAY_PATHS.filter((path) => existsSync(join(ROOT, path)));
  for (const path of overlayPaths) {
    const target = join(snapshot, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, readFileSync(join(ROOT, path)));
  }
  const dependencies = join(ROOT, "node_modules");
  if (!existsSync(dependencies)) throw new Error("canonical core benchmark requires existing node_modules");
  symlinkSync(dependencies, join(snapshot, "node_modules"));
  // Give repoFiles the same tracked inventory semantics as production without
  // touching source history or live checkout state.
  execFileSync("git", ["init", "-q"], { cwd: snapshot, stdio: "ignore" });
  execFileSync("git", ["add", "--", "."], { cwd: snapshot, stdio: "ignore" });
  execFileSync("git", ["-c", "user.name=Blueprint benchmark", "-c", "user.email=benchmark@localhost", "commit", "-qm", "canonical-core-snapshot"], { cwd: snapshot, stdio: "ignore" });
  return { snapshot, overlayPaths, measuredContentSha256: measurementContentSha256(snapshot, [...new Set([...inventory.files, ...overlayPaths])]), elapsedMs: performance.now() - started };
}

async function runBuild(snapshot, args) {
  const started = performance.now();
  const command = ["scripts/blueprint.mjs", "build", "--out", ".agent", ...args];
  const timingsPath = join(snapshot, `.benchmark-stage-timings-${process.pid}-${Date.now()}.json`);
  const child = spawn(process.execPath, command, { cwd: snapshot, env: { ...process.env, BLUEPRINT_LOCAL_BUILD: "1", BLUEPRINT_BENCHMARK_TIMINGS_PATH: timingsPath }, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "", stderr = "", peakRssBytes = 0;
  child.stdout.on("data", (chunk) => { stdout += chunk; }); child.stderr.on("data", (chunk) => { stderr += chunk; });
  const sampleRss = () => {
    try { peakRssBytes = Math.max(peakRssBytes, Number(execFileSync("ps", ["-o", "rss=", "-p", String(child.pid)], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim() || 0) * 1024); } catch { /* process exited */ }
  };
  sampleRss(); const poll = setInterval(sampleRss, 10);
  const result = await new Promise((resolveResult, reject) => child.once("error", reject).once("close", (code, signal) => resolveResult({ code, signal })));
  clearInterval(poll); sampleRss();
  if (result.code !== 0) throw new Error(`canonical core build failed (${result.code ?? result.signal}): ${stderr.slice(-1200)}`);
  if (!peakRssBytes) throw new Error("canonical core RSS sampling produced no observation");
  let stageTimings = { state: "missing" };
  try {
    const parsed = JSON.parse(readFileSync(timingsPath, "utf8"));
    stageTimings = { state: parsed.schema === "blueprint-build-stage-timings-v1" ? "measured" : "invalid", records: parsed.records ?? [], unobservedStages: [{ stage: "graph_persistence", reason: "CLI owns SQLite publication outside static-provider boundary" }] };
  } catch { /* output is optional outside benchmark instrumentation */ }
  rmSync(timingsPath, { force: true });
  return { elapsedMs: performance.now() - started, peakRssBytes, stdout: stdout.trim(), stderr: stderr.trim(), command: [process.execPath, ...command].join(" "), rssProbe: "ps -o rss= -p <pid> (10ms interval)", stageTimings };
}

export function validateCanonicalCoreReport(report) {
  const problems = [];
  if (report?.schema !== REPORT_SCHEMA) problems.push("schema");
  if (!report?.source?.head || !report?.source?.worktree?.state) problems.push("source identity");
  if (!Number.isInteger(report?.inventory?.selectedFiles) || !report?.inventory?.selectionSha256 || !report?.inventory?.trackedContentSha256) problems.push("tracked inventory");
  if (!report?.isolation?.temporarySnapshot || report?.isolation?.liveAgentMutated !== false || !report?.isolation?.measuredContentSha256) problems.push("isolation");
  if (!report?.commands?.inventory || !report?.commands?.snapshot || !report?.commands?.smoke) problems.push("commands");
  if (!report?.host?.node || !report?.host?.platform) problems.push("host");
  for (const metric of ["smoke", "coldBuild", "noChangeBuild"]) {
    const samples = report?.metrics?.[metric]?.samples;
    if (!Array.isArray(samples) || samples.length < 1 || samples.some((sample) => !Number.isFinite(sample.elapsedMs) || sample.elapsedMs <= 0 || !Number.isFinite(sample.peakRssBytes) || sample.peakRssBytes <= 0 || sample.stageTimings?.state !== "measured")) problems.push(`${metric}.samples`);
  }
  return { ok: problems.length === 0, problems };
}

export async function runCanonicalCoreBenchmark({ samples = 1, reportPath = null, keepSnapshot = false } = {}) {
  if (!Number.isInteger(samples) || samples < 1) throw new Error("samples must be a positive integer");
  const inventory = trackedCoreInventory();
  const source = { head: git(["rev-parse", "HEAD"]).trim(), root: ROOT, worktree: worktreeIdentity() };
  const copied = copyTrackedCore(inventory);
  try {
    const smoke = [await runBuild(copied.snapshot, ["--limit", "25"])];
    rmSync(join(copied.snapshot, ".agent"), { recursive: true, force: true });
    const coldBuild = [];
    for (let index = 0; index < samples; index += 1) {
      rmSync(join(copied.snapshot, ".agent"), { recursive: true, force: true });
      coldBuild.push(await runBuild(copied.snapshot, []));
    }
    const noChangeBuild = [];
    for (let index = 0; index < samples; index += 1) noChangeBuild.push(await runBuild(copied.snapshot, []));
    const report = {
      schema: REPORT_SCHEMA,
      command: `node scripts/benchmark-canonical-core.mjs --samples ${samples}${reportPath ? " --report <path>" : ""}`,
      source,
      inventory: { selection: "git ls-files --cached -z; explicit core exclusions", selectedFiles: inventory.files.length, trackedFiles: inventory.trackedFileCount, excludedFiles: inventory.excludedFileCount, selectionSha256: inventory.selectionSha256, trackedContentSha256: inventory.trackedContentSha256, exclusions: EXCLUDED_PREFIXES },
      isolation: { temporarySnapshot: true, snapshotMethod: "copy HEAD-selected tracked files + explicit measurement overlay + temporary git index + node_modules symlink", snapshotPreparationMs: copied.elapsedMs, overlayPaths: copied.overlayPaths, measuredContentSha256: copied.measuredContentSha256, liveAgentMutated: false },
      commands: { inventory: "git ls-files --cached -z", snapshot: "git init -q; git add -- .; git -c user.name=Blueprint\\ benchmark -c user.email=benchmark@localhost commit -qm canonical-core-snapshot", smoke: smoke[0].command, coldBuild: coldBuild.map((item) => item.command), noChangeBuild: noChangeBuild.map((item) => item.command) },
      host: { hostname: os.hostname(), platform: process.platform, arch: process.arch, release: os.release(), node: process.version, git: toolVersion("git"), pnpm: toolVersion("pnpm") },
      samples,
      metrics: {
        smoke: { limit: 25, samples: smoke, p95Ms: p95(smoke.map((item) => item.elapsedMs)), peakRssBytes: Math.max(...smoke.map((item) => item.peakRssBytes)) },
        coldBuild: { samples: coldBuild, p95Ms: p95(coldBuild.map((item) => item.elapsedMs)), peakRssBytes: Math.max(...coldBuild.map((item) => item.peakRssBytes)) },
        noChangeBuild: { samples: noChangeBuild, p95Ms: p95(noChangeBuild.map((item) => item.elapsedMs)), peakRssBytes: Math.max(...noChangeBuild.map((item) => item.peakRssBytes)) },
        stageTimings: { stableStages: ["source_scan", "parse_cache_extraction", "indexed_global_resolution", "identity", "parse_cache_persistence", "treesitter_augmentation", "graph_persistence"], smoke: smoke[0].stageTimings, coldBuild: coldBuild.map((item) => item.stageTimings), noChangeBuild: noChangeBuild.map((item) => item.stageTimings), snapshotPreparationMs: copied.elapsedMs },
      },
    };
    const checked = validateCanonicalCoreReport(report); if (!checked.ok) throw new Error(`invalid canonical core report: ${checked.problems.join(", ")}`);
    if (reportPath) writeFileSync(resolve(reportPath), `${JSON.stringify(report, null, 2)}\n`);
    return report;
  } finally { if (!keepSnapshot) rmSync(copied.snapshot, { recursive: true, force: true }); }
}

function parseArgs(argv) {
  const result = { samples: 1, reportPath: null, keepSnapshot: false };
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--samples") result.samples = Number(argv[++index]);
    else if (argv[index] === "--report") result.reportPath = argv[++index];
    else if (argv[index] === "--keep-snapshot") result.keepSnapshot = true;
    else throw new Error(`unknown argument: ${argv[index]}`);
  }
  return result;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.stdout.write(`${JSON.stringify(await runCanonicalCoreBenchmark(parseArgs(process.argv.slice(2))), null, 2)}\n`);
