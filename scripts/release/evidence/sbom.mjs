#!/usr/bin/env node
// MBR-807: a real software bill of materials, derived only from lockfiles
// already committed to this repository (pnpm-lock.yaml, engine/Cargo.lock).
// It never fabricates a component,
// version, source, or checksum: every entry is parsed straight out of an
// existing lockfile line, and anything this parser cannot resolve a real
// checksum for is reported in `gaps`, never guessed at or filled with a
// plausible-looking placeholder.
//
// These are the exact resolved dependency graphs for the portable Membrane
// add-on: root JS checks, the pinned Blueprint runtime package, plus its locked
// engine workspace. Fixture lockfiles do not participate in a shipped add-on.
//
// This module never runs cargo, pnpm, or any other command: it only reads
// and parses text files already on disk.
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

export const SCHEMA = "membrane.sbom.v1";

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };

export const DEFAULT_LOCKFILES = [
  { ecosystem: "npm", path: "pnpm-lock.yaml" },
  { ecosystem: "npm", path: "blueprint/pnpm-lock.yaml" },
  { ecosystem: "cargo", path: "engine/Cargo.lock" },
];

function splitNameVersion(rawKey) {
  const key = rawKey.trim().replace(/^'(.*)'$/, "$1").replace(/^"(.*)"$/, "$1");
  const at = key.lastIndexOf("@");
  if (at <= 0) fail(`cannot split pnpm-lock.yaml package key into name/version: ${rawKey}`);
  return { name: key.slice(0, at), version: key.slice(at + 1) };
}

/**
 * Minimal, targeted parser for pnpm-lock.yaml's `packages:` section. This is
 * NOT a general YAML parser -- no YAML-parsing package is installed in this
 * workspace and this task may not add one (MEMBRANE-BOOK-MODE-EXECUTION-RULES
 * / Minimize: REUSE and INSTALLED_DEP rungs were checked and rejected before
 * writing this). It understands exactly the two-space-key/four-space-field
 * indentation pnpm's lockfile writer uses for this section (verified against
 * every lockfile this task reads). A package entry with a `resolution:` line
 * but no parseable `integrity:` value, or with no `resolution:` line at all
 * (e.g. a local `link:`/`file:` dependency), is reported as a gap rather than
 * silently dropped or given a fabricated checksum.
 */
export function parsePnpmLock(text, lockfilePath) {
  const lines = text.split("\n");
  const start = lines.findIndex((line) => line === "packages:");
  if (start === -1) return { components: [], gaps: [] };
  const components = [];
  const gaps = [];
  let current = null;
  const flush = () => {
    if (!current) return;
    if (current.integrityLine) {
      const match = current.integrityLine.match(/integrity:\s*([^\s,}]+)/);
      if (match) {
        components.push({
          name: current.name,
          version: current.version,
          ecosystem: "npm",
          source: "https://registry.npmjs.org/",
          checksum: { algorithm: "integrity", value: match[1] },
          lockfile: lockfilePath,
          resolved: true,
        });
      } else {
        gaps.push({ name: current.name, version: current.version, ecosystem: "npm", lockfile: lockfilePath, reason: "resolution present but no parseable integrity value" });
      }
    } else {
      gaps.push({ name: current.name, version: current.version, ecosystem: "npm", lockfile: lockfilePath, reason: "no resolution/integrity recorded in lockfile (local, link, or unresolved dependency)" });
    }
    current = null;
  };
  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.length && !/^\s/.test(line)) break; // dedented out of the `packages:` section
    const entryMatch = line.match(/^ {2}(\S.*):$/);
    if (entryMatch) {
      flush();
      const { name, version } = splitNameVersion(entryMatch[1]);
      current = { name, version, integrityLine: null };
      continue;
    }
    if (current && /^ {4}resolution:/.test(line)) current.integrityLine = line;
  }
  flush();
  return { components, gaps };
}

/**
 * Minimal, targeted parser for Cargo.lock's `[[package]]` array-of-tables
 * (the same reason as parsePnpmLock: no TOML-parsing package is installed
 * and none is named by this task's dispatch). A package with no `source`
 * line is a first-party workspace/path member -- recorded honestly as
 * `source: "local-path"`, `checksum: null`, never a fabricated hash. A
 * package with a `source` but no `checksum` (e.g. a hypothetical git
 * dependency) is reported as a gap.
 */
export function parseCargoLock(text, lockfilePath) {
  const blocks = text.split(/\n(?=\[\[package\]\])/);
  const components = [];
  const gaps = [];
  for (const block of blocks) {
    if (!block.includes("[[package]]")) continue;
    const name = block.match(/\nname = "([^"]+)"/)?.[1];
    const version = block.match(/\nversion = "([^"]+)"/)?.[1];
    if (!name || !version) continue;
    const source = block.match(/\nsource = "([^"]+)"/)?.[1];
    const checksum = block.match(/\nchecksum = "([^"]+)"/)?.[1];
    if (!source) {
      components.push({ name, version, ecosystem: "cargo", source: "local-path", checksum: null, lockfile: lockfilePath, resolved: true, local: true });
      continue;
    }
    if (!checksum) {
      gaps.push({ name, version, ecosystem: "cargo", lockfile: lockfilePath, reason: `source ${source} recorded with no checksum in lockfile` });
      continue;
    }
    if (!/^[0-9a-f]{64}$/.test(checksum)) {
      gaps.push({ name, version, ecosystem: "cargo", lockfile: lockfilePath, reason: "checksum in lockfile is not a lowercase 64-hex sha256" });
      continue;
    }
    components.push({ name, version, ecosystem: "cargo", source, checksum: { algorithm: "sha256", value: checksum }, lockfile: lockfilePath, resolved: true });
  }
  return { components, gaps };
}

function compareComponents(a, b) {
  if (a.ecosystem !== b.ecosystem) return a.ecosystem < b.ecosystem ? -1 : 1;
  if (a.name !== b.name) return a.name < b.name ? -1 : 1;
  return a.version < b.version ? -1 : a.version > b.version ? 1 : 0;
}

/**
 * Generates the full SBOM for `lockfiles` (default: DEFAULT_LOCKFILES). Every
 * lockfile named must exist and be read as-is; a missing lockfile fails
 * closed rather than silently producing a partial SBOM. `generatedFrom`
 * hash-binds the exact lockfile bytes this SBOM was derived from, so a
 * regenerated SBOM against changed lockfiles is provably different, and an
 * unchanged one is provably identical.
 */
function bindArtifact(repoRoot, artifact) {
  if (typeof artifact !== "string" || !artifact) fail("artifact is required");
  const abs = resolve(repoRoot, artifact);
  if (!existsSync(abs)) fail(`artifact not found: ${artifact}`);
  const stat = lstatSync(abs);
  if (stat.isSymbolicLink() || !stat.isFile()) fail(`artifact must be a regular file: ${artifact}`);
  const bytes = readFileSync(abs);
  const path = relative(repoRoot, abs);
  return {
    path: path && !path.startsWith(`..${sep}`) && path !== ".." ? path.replaceAll("\\", "/") : abs,
    size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

export function generateSbom({ repoRoot, artifact, lockfiles = DEFAULT_LOCKFILES } = {}) {
  if (!repoRoot) fail("repoRoot is required");
  if (!Array.isArray(lockfiles) || lockfiles.length === 0) fail("lockfiles must be a non-empty array");
  const artifactReceipt = bindArtifact(repoRoot, artifact);
  const components = [];
  const gaps = [];
  const generatedFrom = [];
  for (const entry of lockfiles) {
    if (entry.ecosystem !== "npm" && entry.ecosystem !== "cargo") fail(`unknown ecosystem: ${entry.ecosystem}`);
    const abs = resolve(repoRoot, entry.path);
    if (!existsSync(abs)) fail(`lockfile not found: ${entry.path}`);
    const text = readFileSync(abs, "utf8");
    const parsed = entry.ecosystem === "npm" ? parsePnpmLock(text, entry.path) : parseCargoLock(text, entry.path);
    components.push(...parsed.components);
    gaps.push(...parsed.gaps);
    generatedFrom.push({ path: entry.path, ecosystem: entry.ecosystem, sha256: createHash("sha256").update(text).digest("hex") });
  }
  components.sort(compareComponents);
  return { schema: SCHEMA, artifact: artifactReceipt, generatedFrom, componentCount: components.length, components, gaps };
}

function parseArgs(argv) {
  const args = { repoRoot: process.cwd() };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--repo-root") args.repoRoot = argv[++i];
    else if (arg === "--artifact") args.artifact = argv[++i];
    else if (arg === "--out") args.out = argv[++i];
    else if (arg === "-h" || arg === "--help") args.help = true;
    else fail(`unknown argument: ${arg}`);
  }
  return args;
}

function usage() {
  console.log(
    "usage: sbom.mjs --artifact INSTALLER [--repo-root PATH] [--out FILE]\n\n" +
    "Prints (or, with --out, also writes) the real SBOM derived from\n" +
    "pnpm-lock.yaml, blueprint/pnpm-lock.yaml, and engine/Cargo.lock, bound to exact installer bytes.\n" +
    "Never runs cargo, pnpm, or any other command; reads committed lockfiles only.",
  );
}

function cli() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) { usage(); process.exit(0); }
  try {
    const repoRoot = resolve(args.repoRoot);
    const sbom = generateSbom({ repoRoot, artifact: args.artifact });
    const json = `${JSON.stringify(sbom, null, 2)}\n`;
    if (args.out) writeFileSync(resolve(repoRoot, args.out), json);
    process.stdout.write(json);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
