#!/usr/bin/env node
// scripts/qualification/cases/ex-windows.mjs — federation-catalog lane case module.
//
// r5.2 amendment (windows-amendment-acceptance.json): EX-01..EX-09 moved out
// of scripts/qualification/cases/pul-windows.mjs because that module's
// federation-catalog touch budget was exhausted. pul-windows.mjs stays
// closed and is not edited by this module; its own EX_01..EX_09 exports
// (a narrower source-tree-only scan) are unaffected and unused here.
//
// Contract: each export is called by scripts/qualification/run.mjs
// runOneRegistryCase as caseFunction({ row, workspaceRoot, profile, platform,
// evidencePath }) and must return { status: "passed"|<anything else>,
// evidenceKind, detail, reason } where evidenceKind is one of run.mjs
// EVIDENCE_KINDS.
//
// Per windows-amendment-acceptance.json's shared EX-01..EX-09 acceptance
// text: "Static and runtime check that the installed candidate contains no
// <forbidden pattern>: crate/dependency inventory, route/operation registry
// and receipts are inspected; any occurrence is a failure regardless of
// test success elsewhere." This module is the real, executable static half
// of that check (source tree + crate manifests/dependency inventory + the
// operation registry + receipts under audit/), runnable today without a
// build. The runtime/installed half over a signed candidate can only be
// closed by the integration owner's installed-path run (executionOwner:
// "Membrane integration owner only"; implementationStatus:
// "IMPLEMENT_THEN_RUN; no runtime result claimed" on every EX row) — this
// module never fabricates that runtime result, it reports the static scan
// as "passed"/"failed" and leaves the runtime half as an explicit
// unclaimed gap in `reason`.
//
// Every exported function accepts an optional `{ root }` or
// `{ workspaceRoot }` (both honored, root wins) so tests can point the scan
// at a fixture tree instead of the live repository — this is how the
// negative controls in ex-windows.test.mjs (if authored) inject the
// forbidden pattern into a fixture root without mutating real source.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

function resolveRoot(context) {
  return (context && context.root) || (context && context.workspaceRoot) || REPO_ROOT;
}

function readFileSafe(root, relPath) {
  const abs = join(root, relPath);
  if (!existsSync(abs)) return null;
  try {
    const st = statSync(abs);
    if (!st.isFile()) return null;
    return readFileSync(abs, "utf8");
  } catch {
    return null;
  }
}

// Crate/dependency inventory: every source file plus every Cargo.toml
// manifest across the engine workspace (dependency inventory lives in
// [dependencies] tables; a forbidden crate name/dependency shows up there
// even with no matching source-level marker).
const SOURCE_SCAN_DIRS = [
  "engine/crates/membrane-core/src",
  "engine/crates/membrane-federation/src",
  "engine/crates/membrane-runtime/src",
  "engine/crates/membrane-protocol/src",
  "engine/crates/membrane-blueprint/src",
  "engine/crates/membrane-client/src",
  "engine/crates/cortex/src",
  "engine/crates/cortex-core/src",
  "engine/crates/membrane-adapt/src",
];
const MANIFEST_FILES = ["engine/Cargo.toml"];
const MANIFEST_GLOB_DIRS = ["engine/crates"];

// Route/operation registry: the golden operations index and its schema are
// the authoritative surface for what operations/routes the installed
// candidate exposes.
const REGISTRY_DIRS = ["schemas/registry/operations", "schemas/registry/hub", "schemas/registry/resources"];

// Receipts: remediation/audit receipts recorded for this repository.
const RECEIPT_DIRS = ["audit/remediation", "audit/session-a", "audit/session-b"];

function walk(root, relDir, matchExt, out) {
  const abs = join(root, relDir);
  if (!existsSync(abs)) return out;
  let entries;
  try {
    entries = readdirSync(abs);
  } catch {
    return out;
  }
  for (const entry of entries) {
    const relPath = join(relDir, entry);
    const absPath = join(root, relPath);
    let st;
    try {
      st = statSync(absPath);
    } catch {
      continue;
    }
    if (st.isDirectory()) {
      walk(root, relPath, matchExt, out);
    } else if (matchExt.test(entry)) {
      // Normalize to forward slashes so relPath is stable across platforms.
      out.push(relPath.split("\\").join("/"));
    }
  }
  return out;
}

function collectManifests(root) {
  const files = [...MANIFEST_FILES];
  for (const dir of MANIFEST_GLOB_DIRS) {
    const abs = join(root, dir);
    if (!existsSync(abs)) continue;
    let crates;
    try {
      crates = readdirSync(abs);
    } catch {
      continue;
    }
    for (const crate of crates) {
      const rel = join(dir, crate, "Cargo.toml").split("\\").join("/");
      if (existsSync(join(root, rel))) files.push(rel);
    }
  }
  return files;
}

function collectScanFiles(root) {
  const sourceFiles = SOURCE_SCAN_DIRS.flatMap((d) => walk(root, d, /\.rs$/, []));
  const manifestFiles = collectManifests(root);
  const registryFiles = REGISTRY_DIRS.flatMap((d) => walk(root, d, /\.json$/, []));
  const receiptFiles = RECEIPT_DIRS.flatMap((d) => walk(root, d, /\.json$/, []));
  return {
    sourceFiles,
    manifestFiles,
    registryFiles,
    receiptFiles,
    all: [...sourceFiles, ...manifestFiles, ...registryFiles, ...receiptFiles],
  };
}

// Real, executable static exclusion check across source + crate/dependency
// inventory + route/operation registry + receipts, per the shared EX-01..
// EX-09 acceptance text in windows-amendment-acceptance.json.
function exclusionCheck(id, title, patterns, context) {
  const root = resolveRoot(context);
  const { sourceFiles, manifestFiles, registryFiles, receiptFiles, all } = collectScanFiles(root);
  const hits = [];
  for (const relPath of all) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const pattern of patterns) {
      if (pattern.test(content)) hits.push({ file: relPath, pattern: String(pattern) });
    }
  }
  const scanned = {
    source: sourceFiles.length,
    manifests: manifestFiles.length,
    registry: registryFiles.length,
    receipts: receiptFiles.length,
    total: all.length,
  };
  const staticPass = hits.length === 0;
  let installedInventory = { status: "unavailable", reason: "MEMBRANE_QUALIFICATION_INSTALLED_ROOT is not configured" };
  const installedRoot = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  const installedExe = installedRoot && join(resolve(installedRoot), "membrane.exe");
  if (installedExe && existsSync(installedExe)) {
    try {
      const buildInfo = JSON.parse(execFileSync(installedExe, ["cli", "build-info"], { cwd: root, encoding: "utf8", windowsHide: true }));
      installedInventory = { status: "observed", runtimeOrigin: buildInfo.runtimeOrigin ?? buildInfo.runtime_origin, generation: buildInfo.releaseGeneration ?? buildInfo.release_generation ?? buildInfo.generation };
    } catch (error) { installedInventory = { status: "failed", reason: error.message }; }
  }
  return {
    status: staticPass ? "insufficient" : "failed",
    evidenceKind: "source",
    detail: { id, title, scanned, hits, installedInventory },
    reason: staticPass
      ? id +
        ": " +
        title +
        " — no forbidden pattern found across " +
        all.length +
        " scanned file(s) (source + crate/dependency inventory + route/operation registry + receipts). Static half only: the installed-candidate runtime half of this check remains unclaimed (executionOwner: Membrane integration owner only; implementationStatus: IMPLEMENT_THEN_RUN, no runtime result claimed here)."
      : id +
        ": " +
        title +
        " — forbidden pattern present at " +
        hits.map((h) => h.file).join(", ") +
        "; this is a failure regardless of test success elsewhere.",
  };
}

export function EX_01(context) {
  return exclusionCheck(
    "EX-01",
    "no second graph/search/impact/planner/memory/vector/Dream service",
    [
      /\bDream(Service|Engine|Planner)\b/,
      /second[_\s-]?(graph|search|impact|planner|memory|vector)[_\s-]?service/i,
      /struct\s+\w*(GraphService|SearchService|ImpactService|VectorService)\w*/,
    ],
    context,
  );
}
export function EX_02(context) {
  return exclusionCheck(
    "EX-02",
    "no per-client semantic authority",
    [/per[_\s-]?client[_\s-]?(semantic[_\s-]?)?authority/i, /ClientSemanticAuthority/],
    context,
  );
}
export function EX_03(context) {
  return exclusionCheck(
    "EX-03",
    "no graph mutation authority in Membrane",
    [/graph[_\s-]?mutation[_\s-]?authority/i, /fn\s+mutate_graph\b/, /GraphMutationAuthority/],
    context,
  );
}
export function EX_04(context) {
  return exclusionCheck(
    "EX-04",
    "no Markdown graph store",
    [/markdown[_\s-]?graph[_\s-]?store/i, /MarkdownGraphStore/],
    context,
  );
}
export function EX_05(context) {
  return exclusionCheck(
    "EX-05",
    "no unverified LLM truth admitted as fact",
    [/unverified[_\s-]?(llm[_\s-]?)?truth/i, /admit_as_fact\s*\(\s*llm/i, /admit_unverified_llm/i],
    context,
  );
}
export function EX_06(context) {
  return exclusionCheck(
    "EX-06",
    "no scalar-trust replacement of typed authority",
    [/scalar[_\s-]?trust[_\s-]?(score)?\s*(:|as)\s*(f32|f64)/i, /replace.*typed[_\s-]?authority.*scalar/i],
    context,
  );
}
export function EX_07(context) {
  return exclusionCheck(
    "EX-07",
    "no arbitrary truth expiration",
    [/arbitrary[_\s-]?(truth[_\s-]?)?expir/i, /expire_truth_after\s*\(/i],
    context,
  );
}
export function EX_08(context) {
  return exclusionCheck(
    "EX-08",
    "no speculative memory branching/merging",
    [/speculative[_\s-]?(memory[_\s-]?)?(branch|merge)/i, /SpeculativeBranch/, /speculative_merge\s*\(/i],
    context,
  );
}
export function EX_09(context) {
  return exclusionCheck(
    "EX-09",
    "no automatic paid freshness or recomputation calls",
    [/automatic[_\s-]?paid[_\s-]?(freshness|recomput)/i, /auto_paid_refresh\s*\(/i],
    context,
  );
}

export const EX_CASES = { EX_01, EX_02, EX_03, EX_04, EX_05, EX_06, EX_07, EX_08, EX_09 };
export default EX_CASES;
