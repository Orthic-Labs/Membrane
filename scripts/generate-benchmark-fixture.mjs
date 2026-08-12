#!/usr/bin/env node
// Deterministic, disposable benchmark source corpus.  Generated trees stay out
// of git; this module is the committed identity and reconstruction recipe.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

export const FIXTURE_VERSION = 1;
export const FIXTURE_SEED = "cortex-storage-v1";
export const DEFAULT_FIXTURE_FILES = 550;
export const LARGE_FIXTURE_FILES = 5000;
export const FIXTURE_MARKER = ".cortex-benchmark-fixture.json";

function sourceFor(index) {
  const group = String(Math.floor(index / 25)).padStart(3, "0");
  const name = `benchmarkSymbol${String(index).padStart(5, "0")}`;
  return `// ${FIXTURE_SEED}; fixture=${FIXTURE_VERSION}; file=${index}\nexport const ${name} = ${index};\nexport function use${name}() { return ${name}; }\n`;
}

export function fixtureFilePath(index) {
  const group = String(Math.floor(index / 25)).padStart(3, "0");
  return `src/modules/group-${group}/unit-${String(index).padStart(5, "0")}.mjs`;
}

export function fixtureIdentity(files) {
  const hash = createHash("sha256");
  for (let index = 0; index < files; index += 1) {
    hash.update(fixtureFilePath(index));
    hash.update("\0");
    hash.update(sourceFor(index));
    hash.update("\0");
  }
  return Object.freeze({
    id: `cortex-storage-${files}-v${FIXTURE_VERSION}`,
    fixtureVersion: FIXTURE_VERSION,
    seed: FIXTURE_SEED,
    files,
    updatePaths: Math.min(100, files),
    contentSha256: hash.digest("hex"),
  });
}

function safeReplace(out) {
  if (!existsSync(out)) return;
  const marker = join(out, FIXTURE_MARKER);
  if (!existsSync(marker)) throw new Error(`refusing to replace unmarked directory: ${out}`);
  rmSync(out, { recursive: true, force: true });
}

export function generateBenchmarkFixture({ outDir, files = DEFAULT_FIXTURE_FILES, force = false } = {}) {
  if (!Number.isInteger(files) || files < 1) throw new Error("files must be a positive integer");
  const out = resolve(outDir ?? "");
  if (!outDir) throw new Error("outDir is required");
  if (existsSync(out)) {
    const marker = join(out, FIXTURE_MARKER);
    if (!existsSync(marker) && readdirSync(out).length === 0) {
      // mkdtemp() deliberately creates an empty directory for disposable runs.
    } else {
    if (!force && existsSync(marker)) {
      const existing = JSON.parse(readFileSync(marker, "utf8"));
      if (existing.contentSha256 === fixtureIdentity(files).contentSha256) return existing;
    }
    if (!force) throw new Error(`fixture output exists: ${out}; use --force only for a marked fixture`);
    safeReplace(out);
    }
  }
  mkdirSync(out, { recursive: true });
  for (let index = 0; index < files; index += 1) {
    const relative = fixtureFilePath(index);
    const target = join(out, relative);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, sourceFor(index));
  }
  const identity = fixtureIdentity(files);
  writeFileSync(join(out, FIXTURE_MARKER), `${JSON.stringify(identity, null, 2)}\n`);
  return identity;
}

function parseArgs(argv) {
  const result = { files: DEFAULT_FIXTURE_FILES, force: false, outDir: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--out") result.outDir = argv[++index];
    else if (arg === "--files") result.files = Number(argv[++index]);
    else if (arg === "--force") result.force = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  return result;
}

const invoked = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) {
  const identity = generateBenchmarkFixture(parseArgs(process.argv.slice(2)));
  process.stdout.write(`${JSON.stringify(identity)}\n`);
}
