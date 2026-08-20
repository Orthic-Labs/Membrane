#!/usr/bin/env node
/** Generate Membrane's install workspace package from source authority. */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const SCHEMA_VERSION = "membrane-install-workspace-v1";
export const PACKAGE_VERSION = "1.0.0";
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
export const DEFAULT_SOURCE = resolve(SCRIPT_DIR, "../install/workspace");
export const DEFAULT_DIST = resolve(SCRIPT_DIR, "../dist/install/workspace");
export const DEFAULT_MANIFEST = resolve(SCRIPT_DIR, "../dist/install/workspace-manifest.json");

const RETIRED = /(?:crypt_service|crypt-service|orthic_manifest|orthic-manifest)/i;
const ignored = new Set(["__pycache__", ".pytest_cache", ".DS_Store"]);

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function filesUnder(root) {
  const files = [];
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (ignored.has(entry.name)) continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) files.push(path);
    }
  }
  visit(root);
  return files.sort((a, b) => relative(root, a).localeCompare(relative(root, b)));
}

function canonicalFiles(root) {
  return filesUnder(root).map((path) => {
    const bytes = readFileSync(path);
    const rel = relative(root, path).replaceAll("\\", "/");
    if (RETIRED.test(rel) || RETIRED.test(bytes.toString("utf8"))) {
      throw new Error(`retired_workspace_artifact:${rel}`);
    }
    if (rel.split("/").some((part) => part.startsWith("test_"))) {
      throw new Error(`runtime_workspace_test_artifact:${rel}`);
    }
    return { path: rel, sha256: sha256(bytes), bytes: bytes.byteLength };
  });
}

function pruneIgnored(root) {
  if (!existsSync(root)) return;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (ignored.has(entry.name)) rmSync(path, { recursive: true, force: true });
    else if (entry.isDirectory()) pruneIgnored(path);
  }
}

function digestFor(files, runtime) {
  return sha256(Buffer.from(JSON.stringify({ schemaVersion: SCHEMA_VERSION, packageVersion: PACKAGE_VERSION, files, runtime })));
}

export function buildManifest(sourceRoot) {
  if (!existsSync(sourceRoot) || !statSync(sourceRoot).isDirectory()) throw new Error(`workspace_package_missing:${sourceRoot}`);
  const files = canonicalFiles(sourceRoot);
  if (!files.some(({ path }) => path === "cortex_service.py")) throw new Error("workspace_package_missing:cortex_service.py");
  const runtime = { python: ">=3.11", dependencies: [] };
  return {
    schemaVersion: SCHEMA_VERSION,
    packageVersion: PACKAGE_VERSION,
    source: "membrane/install/workspace",
    generated: "source-to-dist",
    runtime,
    files,
    packageSha256: digestFor(files, runtime),
  };
}

function parseArgs(argv) {
  const args = { source: DEFAULT_SOURCE, dist: DEFAULT_DIST, manifest: DEFAULT_MANIFEST, check: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--check") args.check = true;
    else if (arg === "--source") args.source = resolve(argv[++i]);
    else if (arg === "--dist") args.dist = resolve(argv[++i]);
    else if (arg === "--manifest") args.manifest = resolve(argv[++i]);
    else throw new Error(`unknown_argument:${arg}`);
  }
  return args;
}

function readJson(path) {
  try { return JSON.parse(readFileSync(path, "utf8")); }
  catch (error) { throw new Error(`workspace_manifest_invalid:${path}:${error.message}`); }
}

export function checkManifest(manifest, distRoot) {
  if (manifest.schemaVersion !== SCHEMA_VERSION) return ["workspace_manifest_schema_mismatch"];
  if (!Array.isArray(manifest.files)) return ["workspace_manifest_files_missing"];
  const expected = new Map(manifest.files.map((entry) => [entry.path, entry]));
  const actual = new Map(canonicalFiles(distRoot).map((entry) => [entry.path, entry]));
  const errors = [];
  for (const path of expected.keys()) {
    if (!actual.has(path)) errors.push(`workspace_package_missing:${path}`);
    else if (JSON.stringify(expected.get(path)) !== JSON.stringify(actual.get(path))) errors.push(`workspace_package_mismatch:${path}`);
  }
  for (const path of actual.keys()) if (!expected.has(path)) errors.push(`workspace_package_extra:${path}`);
  if (manifest.packageSha256 !== digestFor(manifest.files, manifest.runtime)) errors.push("workspace_manifest_digest_mismatch");
  return errors;
}

export function checkSourceManifest(manifest, sourceRoot) {
  const current = buildManifest(sourceRoot);
  const errors = [];
  if (manifest.packageSha256 !== current.packageSha256) errors.push("workspace_source_drift");
  if (JSON.stringify(manifest.files) !== JSON.stringify(current.files)) errors.push("workspace_source_manifest_mismatch");
  return errors;
}

export function generate({ source = DEFAULT_SOURCE, dist = DEFAULT_DIST, manifestPath = DEFAULT_MANIFEST } = {}) {
  const built = buildManifest(source);
  mkdirSync(dist, { recursive: true });
  pruneIgnored(dist);
  for (const path of filesUnder(dist)) {
    const rel = relative(dist, path).replaceAll("\\", "/");
    if (!built.files.some((entry) => entry.path === rel)) unlinkSync(path);
  }
  for (const entry of built.files) {
    const target = join(dist, entry.path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, readFileSync(join(source, entry.path)));
  }
  mkdirSync(dirname(manifestPath), { recursive: true });
  writeFileSync(manifestPath, `${JSON.stringify(built, null, 2)}\n`);
  return built;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const manifest = buildManifest(args.source);
    if (args.check) {
      const diskManifest = readJson(args.manifest);
      const errors = [...checkSourceManifest(diskManifest, args.source), ...checkManifest(diskManifest, args.dist)];
      if (errors.length) throw new Error(errors.join("\n"));
      console.log(JSON.stringify({ ok: true, manifest: args.manifest, packageSha256: manifest.packageSha256 }));
    } else {
      generate(args);
      console.log(JSON.stringify({ ok: true, manifest: args.manifest, packageSha256: manifest.packageSha256 }));
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
