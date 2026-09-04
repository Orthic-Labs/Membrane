#!/usr/bin/env node
// Phase 7.5 — randomized test-order runner.
//
// Defect 18 records two production lock deaths that `test:all
// --test-concurrency=1` did not catch because the SERIAL suite runs tests
// in deterministic filesystem order and any race that depends on a
// particular ordering is invisible. Randomizing the file order — and
// running with the SAME concurrency=1 floor — exposes order-dependent
// failures (shared-tempdir collisions, leaked handles, port reuse) without
// throwing away the determinism that makes the serial suite trustworthy.
//
// Usage:  node scripts/test-random.mjs [--seed=N] [--runs=N] [--batch-size=N]
//         node scripts/test-random.mjs --ordered [--root-only]
//         pnpm test:random           # one randomized run, default seed
//
// Ordered mode powers the deterministic suites. Random mode remains a
// separate CI gate for order-dependent failures.

import { readdirSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");
const TESTS = join(ROOT, "tests");
// Product packaging and installed-release rehearsals are separate from the
// no-build engineering lane. Report every omitted file instead of calling a
// subset the full release suite. Default test behavior is unchanged.
export const BUILD_REQUIRED_TESTS = Object.freeze({
  "tests/clean-host-smoke.test.mjs": "builds release candidates and rehearses installed packages",
  "tests/release-candidate.test.mjs": "builds release candidates and package archives",
  "tests/runtime-bundle.test.mjs": "stages a bundled product runtime",
});

export function selectTestFiles(files, { noBuild = false } = {}) {
  const included = [], excluded = [];
  for (const path of files) {
    const reason = noBuild ? BUILD_REQUIRED_TESTS[path.replaceAll("\\", "/")] : null;
    if (reason) excluded.push({ path, reason });
    else included.push(path);
  }
  return { included, excluded };
}

const ISOLATED_TESTS = new Set([
  "treesitter-parity.test.mjs",
  "treesitter-provider.test.mjs",
  "watch-supervisor.test.mjs",
  "watchman.test.mjs",
]);

function parseArgs(argv) {
  const args = { seed: null, runs: 1, batchSize: 32, ordered: false, rootOnly: false, noBuild: false, keepGoing: false };
  for (const arg of argv.slice(2)) {
    if (arg.startsWith("--seed=")) args.seed = Number(arg.slice("--seed=".length));
    else if (arg.startsWith("--runs=")) args.runs = Number(arg.slice("--runs=".length));
    else if (arg.startsWith("--batch-size=")) args.batchSize = Number(arg.slice("--batch-size=".length));
    else if (arg === "--ordered") args.ordered = true;
    else if (arg === "--root-only") args.rootOnly = true;
    else if (arg === "--no-build") args.noBuild = true;
    else if (arg === "--keep-going") args.keepGoing = true;
  }
  if (!Number.isInteger(args.runs) || args.runs < 1) throw new Error("--runs must be a positive integer");
  if (!Number.isInteger(args.batchSize) || args.batchSize < 1) throw new Error("--batch-size must be a positive integer");
  return args;
}

function collectTests(dir, recursive = true) {
  const entries = readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = join(dir, entry.name);
    if (entry.isDirectory() && recursive) {
      files.push(...collectTests(full, true));
    } else if (entry.isFile() && entry.name.endsWith(".test.mjs")) {
      files.push(full);
    }
  }
  return files;
}

// xorshift32 — small deterministic PRNG so a recorded seed reproduces a
// specific run. `Math.random()` is not reproducible across Node versions.
function makeRng(seed) {
  let state = seed >>> 0 || 1;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 0x1_0000_0000;
  };
}

function fisherYates(arr, rng) {
  for (let i = arr.length - 1; i > 0; i -= 1) {
    const j = Math.floor(rng() * (i + 1));
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
}

function run() {
  const args = parseArgs(process.argv);
  const selection = selectTestFiles(collectTests(TESTS, !args.rootOnly).map((path) => relative(ROOT, path)), args);
  const allFiles = selection.included.map((path) => join(ROOT, path));
  for (const excluded of selection.excluded) process.stdout.write(`[test-random] excluded(no-build): ${excluded.path}: ${excluded.reason}\n`);
  let totalFail = 0;
  for (let runIndex = 0; runIndex < args.runs; runIndex += 1) {
    const seed = args.seed ?? (Math.floor(Math.random() * 0xffffffff) || 1);
    const order = args.ordered
      ? [...allFiles].sort((left, right) => left.localeCompare(right))
      : fisherYates([...allFiles], makeRng(seed));
    const relativePaths = order.map((full) => relative(ROOT, full));
    const batches = [];
    let batch = [];
    for (const path of relativePaths) {
      if (ISOLATED_TESTS.has(basename(path))) {
        if (batch.length > 0) batches.push(batch);
        batches.push([path]);
        batch = [];
      } else {
        batch.push(path);
        if (batch.length === args.batchSize) { batches.push(batch); batch = []; }
      }
    }
    if (batch.length > 0) batches.push(batch);
    process.stdout.write(`[test-random] run ${runIndex + 1}/${args.runs} seed=${seed} files=${relativePaths.length} batches=${batches.length}\n`);
    process.stdout.write(`[test-random] order: ${relativePaths.join(" ")}\n`);
    for (let batchIndex = 0; batchIndex < batches.length; batchIndex += 1) {
      const batch = batches[batchIndex];
      process.stdout.write(`[test-random] batch ${batchIndex + 1}/${batches.length} files=${batch.length}\n`);
      const result = spawnSync(process.execPath, ["--test", "--test-concurrency=1", ...batch], {
        cwd: ROOT,
        stdio: "inherit",
        encoding: "utf8",
        timeout: 600_000,
      });
      if (result.status !== 0) {
        process.stderr.write(`[test-random] run ${runIndex + 1} batch ${batchIndex + 1} failed with status=${result.status} error=${result.error?.code ?? "none"}\n`);
        totalFail += 1;
        if (!args.keepGoing) break;
      }
    }
  }
  if (totalFail > 0) {
    process.stderr.write(`[test-random] ${totalFail}/${args.runs} runs failed\n`);
    process.exit(1);
  }
  process.stdout.write(`[test-random] all ${args.runs} runs passed\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try { run(); } catch (error) { process.stderr.write(`${error.stack ?? error.message}\n`); process.exit(1); }
}
