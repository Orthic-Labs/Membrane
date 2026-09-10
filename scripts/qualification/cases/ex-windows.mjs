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

// ---------------------------------------------------------------------------
// Installed-candidate runtime half.
//
// The static half above proves the source tree, crate manifests, operation
// registry, and receipts carry no forbidden pattern. The runtime half proves
// the same absence against the actual INSTALLED product named by
// MEMBRANE_QUALIFICATION_INSTALLED_ROOT: its shipped executables (binary
// string scan — the exact bytes a user runs, not source), its shipped
// mcp/*.mjs|.cjs surface and JSON registries (mcp.json, plugin.json,
// release.json), and — for the two acceptance rows that name a concrete
// observable surface (EX-01: no second registered/listening service; EX-09:
// no automatic paid-network egress) — a live probe of the installed
// candidate's declared service identity, running processes, and scheduled
// tasks. It never starts, stops, installs, or mutates the installed product;
// every probe is read-only (`membrane.exe status --dry-run --bindings-only`,
// `tasklist`, `schtasks /query`). When a required probe cannot be completed
// (root not configured, executable missing, host tool unavailable) this
// reports a typed insufficient reason naming exactly what is missing — it
// never fabricates a pass.

const CANONICAL_EXECUTABLES = ["cortex.exe", "membrane.exe", "membrane-daemon.exe", "membrane-hub.exe", "membrane-tray.exe"];
const CANONICAL_SERVICE_ID = "membrane-hub";

// Literal ASCII byte sequences to search for directly inside the shipped
// executables. Regexes cannot run against compiled binaries; a plain
// substring search over the raw bytes is the real analogue of the source
// pattern scan for a compiled artifact (a debug/panic/log string or a
// non-stripped symbol name for a forbidden construct would appear this way).
const BINARY_LITERALS_BY_ID = {
  "EX-01": ["DreamService", "DreamEngine", "DreamPlanner", "GraphService", "SearchService", "ImpactService", "VectorService"],
  "EX-02": ["ClientSemanticAuthority", "per_client_semantic_authority"],
  "EX-03": ["GraphMutationAuthority", "mutate_graph"],
  "EX-04": ["MarkdownGraphStore", "markdown_graph_store"],
  "EX-05": ["admit_unverified_llm", "AdmitUnverifiedLlmTruth"],
  "EX-06": ["ScalarTrustScore", "scalar_trust_score"],
  "EX-07": ["expire_truth_after", "ArbitraryTruthExpiration"],
  "EX-08": ["SpeculativeBranch", "speculative_merge"],
  "EX-09": ["auto_paid_refresh", "AutomaticPaidFreshness"],
};

function resolveInstalledRoot(context) {
  const fromContext = context && (context.installedRoot || context.installedInventoryRoot);
  if (fromContext) return resolve(fromContext);
  const envRoot = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  return envRoot ? resolve(envRoot) : null;
}

function safeJsonParse(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function collectInstalledInventory(root) {
  let topLevel = [];
  try {
    topLevel = readdirSync(root);
  } catch {
    topLevel = [];
  }
  const executables = topLevel.filter((f) => /\.exe$/i.test(f));
  const mcpJsonText = readFileSafe(root, "mcp.json");
  const pluginJsonText = readFileSafe(root, "plugin.json");
  const releaseJsonText = readFileSafe(root, "release.json");
  const mcpDirFiles = walk(root, "mcp", /\.(mjs|cjs|js|json)$/, []);
  return {
    root,
    topLevel,
    executables,
    mcpJson: mcpJsonText ? safeJsonParse(mcpJsonText) : null,
    hasPluginJson: pluginJsonText !== null,
    hasReleaseJson: releaseJsonText !== null,
    mcpDirFiles,
  };
}

function summarizeInventory(inventory) {
  return {
    root: inventory.root,
    executables: inventory.executables,
    mcpServerKeys: inventory.mcpJson && inventory.mcpJson.mcpServers ? Object.keys(inventory.mcpJson.mcpServers) : null,
    hasPluginJson: inventory.hasPluginJson,
    hasReleaseJson: inventory.hasReleaseJson,
    mcpDirFileCount: inventory.mcpDirFiles.length,
  };
}

// Scans the installed candidate's own shipped JS/JSON surface (not the
// source tree) for the same textual patterns as the static half.
function scanInstalledText(root, inventory, patterns) {
  const files = [...inventory.mcpDirFiles, "mcp.json", "plugin.json", "release.json"];
  const hits = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const pattern of patterns) {
      if (pattern.test(content)) hits.push({ file: relPath, pattern: String(pattern) });
    }
  }
  return hits;
}

// Scans the raw bytes of every shipped executable for literal forbidden
// identifiers. Read-only: opens each file for reading only.
function scanInstalledBinaries(root, executables, literals) {
  const hits = [];
  if (!literals || literals.length === 0) return hits;
  for (const exe of executables) {
    let buf;
    try {
      buf = readFileSync(join(root, exe));
    } catch {
      continue;
    }
    for (const literal of literals) {
      if (buf.includes(Buffer.from(literal, "utf8"))) hits.push({ file: exe, pattern: `binary:${literal}` });
    }
  }
  return hits;
}

// Read-only probe of the installed candidate's declared service identity.
// `status --dry-run --bindings-only` inspects and reconciles client bindings
// without launching, stopping, or mutating anything (per its own --help
// text); it never starts Hub or a background service.
function probeServiceIdentity(root) {
  const exePath = join(root, "membrane.exe");
  if (!existsSync(exePath)) {
    return { status: "unavailable", reason: "membrane.exe not found at installed root" };
  }
  try {
    const out = execFileSync(exePath, ["status", "--dry-run", "--bindings-only"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
    const parsed = safeJsonParse(out);
    if (!parsed) return { status: "failed", reason: "membrane.exe status did not return parseable JSON" };
    return {
      status: "observed",
      serviceId: parsed.service?.serviceId ?? null,
      port: parsed.service?.port ?? null,
      state: parsed.service?.state ?? null,
    };
  } catch (error) {
    return { status: "failed", reason: error.message };
  }
}

function parseTasklistCsv(csvText) {
  return csvText
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0)
    .map((line) => line.split('","').map((cell) => cell.replace(/^"|"$/g, "")));
}

// Read-only process enumeration: proves no non-canonical membrane/graph/
// search/memory-shaped executable is running alongside the canonical set.
function probeRunningProcesses() {
  try {
    const out = execFileSync("tasklist", ["/FO", "CSV", "/NH"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
    const rows = parseTasklistCsv(out);
    const relevant = rows.filter((cols) => /membrane|cortex|dream|graphservice|searchservice|impactservice|vectorservice/i.test(cols[0] || ""));
    const nonCanonicalNamedProcesses = relevant
      .map((cols) => cols[0])
      .filter((name) => !CANONICAL_EXECUTABLES.some((exe) => exe.toLowerCase() === (name || "").toLowerCase()));
    return { status: "observed", nonCanonicalNamedProcesses: [...new Set(nonCanonicalNamedProcesses)] };
  } catch (error) {
    return { status: "failed", reason: error.message };
  }
}

// Read-only scheduled-task enumeration: EX-09's acceptance is "no automatic
// paid freshness or recomputation calls" — a standing scheduled task is the
// concrete Windows-observable surface for an "automatic" call the product
// itself did not just make interactively.
function probeScheduledTasks() {
  try {
    const out = execFileSync("schtasks", ["/query", "/fo", "CSV", "/nh"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
    const rows = parseTasklistCsv(out);
    const membraneTasks = rows.map((cols) => cols[0]).filter((name) => /membrane|cortex/i.test(name || ""));
    return { status: "observed", membraneTasks: [...new Set(membraneTasks)] };
  } catch (error) {
    return { status: "failed", reason: error.message };
  }
}

// Combines the installed-candidate probes into one verdict per EX case.
// Returns { available:false, reason } when the installed root itself is
// absent; otherwise { available:true, insufficient, passed, reason, detail }.
function computeInstalledRuntime(id, patterns, context) {
  const installedRoot = resolveInstalledRoot(context);
  if (!installedRoot || !existsSync(installedRoot)) {
    return { available: false, reason: `MEMBRANE_QUALIFICATION_INSTALLED_ROOT is not configured or does not exist (got ${installedRoot ?? "unset"})` };
  }
  const inventory = collectInstalledInventory(installedRoot);
  if (inventory.executables.length === 0) {
    return { available: false, reason: `installed root ${installedRoot} has no executables; cannot probe the installed candidate` };
  }

  const hits = [
    ...scanInstalledText(installedRoot, inventory, patterns),
    ...scanInstalledBinaries(installedRoot, inventory.executables, BINARY_LITERALS_BY_ID[id]),
  ];
  const extra = {};
  let missing = null;

  if (id === "EX-01") {
    // Cheap, file-only structural checks first: these are deterministic from
    // the installed root's own shipped files and never require a live probe
    // to catch a second-service condition.
    const extraExecutables = inventory.executables.filter((exe) => !CANONICAL_EXECUTABLES.some((c) => c.toLowerCase() === exe.toLowerCase()));
    const mcpServerKeys = inventory.mcpJson && inventory.mcpJson.mcpServers ? Object.keys(inventory.mcpJson.mcpServers) : null;
    extra.extraExecutables = extraExecutables;
    extra.mcpServerKeys = mcpServerKeys;
    if (mcpServerKeys === null) {
      missing = "installed mcp.json is missing or has no mcpServers map";
    } else {
      if (extraExecutables.length > 0) hits.push({ file: extraExecutables.join(","), pattern: "non-canonical installed executable" });
      if (mcpServerKeys.length !== 1 || mcpServerKeys[0] !== "membrane") {
        hits.push({ file: "mcp.json", pattern: `non-canonical mcpServers entry set [${mcpServerKeys.join(",")}]` });
      }
      // Only reach for the live, host-dependent probes once the cheap
      // structural checks are clean — a structural hit above is already a
      // conclusive failure and does not need a live probe to confirm it.
      if (hits.length === 0) {
        const serviceProbe = probeServiceIdentity(installedRoot);
        const processProbe = probeRunningProcesses();
        extra.serviceProbe = serviceProbe;
        extra.processProbe = processProbe;
        if (serviceProbe.status !== "observed") {
          missing = `installed service-identity probe (membrane.exe status --dry-run) unavailable: ${serviceProbe.reason}`;
        } else if (processProbe.status !== "observed") {
          missing = `installed process enumeration (tasklist) unavailable: ${processProbe.reason}`;
        } else {
          if (serviceProbe.serviceId !== CANONICAL_SERVICE_ID) {
            hits.push({ file: "membrane.exe status", pattern: `unexpected serviceId ${serviceProbe.serviceId}` });
          }
          if (processProbe.nonCanonicalNamedProcesses.length > 0) {
            hits.push({ file: "tasklist", pattern: `non-canonical running process(es): ${processProbe.nonCanonicalNamedProcesses.join(",")}` });
          }
        }
      }
    }
  }

  if (id === "EX-09") {
    const scheduledProbe = probeScheduledTasks();
    extra.scheduledProbe = scheduledProbe;
    if (scheduledProbe.status !== "observed") {
      missing = `installed scheduled-task enumeration (schtasks) unavailable: ${scheduledProbe.reason}`;
    } else if (scheduledProbe.membraneTasks.length > 0) {
      hits.push({ file: "schtasks", pattern: `unexpected standing scheduled task(s): ${scheduledProbe.membraneTasks.join(" | ")}` });
    }
  }

  const detail = { installedRoot, inventory: summarizeInventory(inventory), hits, extra };
  if (missing) return { available: true, insufficient: true, reason: missing, detail };
  return { available: true, insufficient: false, passed: hits.length === 0, detail };
}

// Real, executable static exclusion check across source + crate/dependency
// inventory + route/operation registry + receipts, per the shared EX-01..
// EX-09 acceptance text in windows-amendment-acceptance.json, combined with
// the installed-candidate runtime half above.
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
  const installedRootForBuildInfo = resolveInstalledRoot(context);
  const installedExe = installedRootForBuildInfo && join(installedRootForBuildInfo, "membrane.exe");
  if (installedExe && existsSync(installedExe)) {
    try {
      const buildInfo = JSON.parse(execFileSync(installedExe, ["cli", "build-info"], { cwd: root, encoding: "utf8", windowsHide: true }));
      installedInventory = { status: "observed", runtimeOrigin: buildInfo.runtimeOrigin ?? buildInfo.runtime_origin, generation: buildInfo.releaseGeneration ?? buildInfo.release_generation ?? buildInfo.generation };
    } catch (error) { installedInventory = { status: "failed", reason: error.message }; }
  }

  // A forbidden static pattern is always a hard failure — the runtime half
  // is never consulted to override it, and every existing negative control
  // that injects a static-source fixture hit keeps failing exactly as before.
  if (!staticPass) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: { id, title, scanned, hits, installedInventory },
      reason: `${id}: ${title} — forbidden pattern present at ${hits.map((h) => h.file).join(", ")}; this is a failure regardless of test success elsewhere.`,
    };
  }

  const runtime = computeInstalledRuntime(id, patterns, context);
  const staticPassedNote = `${id}: ${title} — no forbidden pattern found across ${all.length} scanned file(s) (source + crate/dependency inventory + route/operation registry + receipts).`;

  if (!runtime.available) {
    return {
      status: "insufficient",
      evidenceKind: "source",
      detail: { id, title, scanned, hits, installedInventory, installedRuntime: { available: false, reason: runtime.reason } },
      reason: `${staticPassedNote} Static half only: the installed-candidate runtime half remains unclaimed — ${runtime.reason}.`,
    };
  }

  if (runtime.insufficient) {
    return {
      status: "insufficient",
      evidenceKind: "source",
      detail: { id, title, scanned, hits, installedInventory, installedRuntime: runtime.detail },
      reason: `${staticPassedNote} Runtime half insufficient: ${runtime.reason}.`,
    };
  }

  if (!runtime.passed) {
    return {
      status: "failed",
      evidenceKind: "installed",
      detail: { id, title, scanned, hits, installedInventory, installedRuntime: runtime.detail },
      reason: `${id}: ${title} — installed-candidate runtime evidence found a forbidden condition: ${runtime.detail.hits.map((h) => `${h.file} (${h.pattern})`).join("; ")}; this is a failure regardless of test success elsewhere.`,
    };
  }

  return {
    status: "passed",
    evidenceKind: "installed",
    detail: { id, title, scanned, hits, installedInventory, installedRuntime: runtime.detail },
    reason: `${staticPassedNote} Runtime half also passed: the installed candidate at ${runtime.detail.installedRoot} was probed directly (shipped executables/JS surface scanned for the forbidden pattern, plus the case-specific installed-service/process/schedule probe) and shows no occurrence.`,
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
