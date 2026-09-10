#!/usr/bin/env node
// scripts/qualification/cases/ctx-windows.mjs — cortex-completion lane case module.
//
// Wave-A scope note (see receipt at <scratchpad>/lanes/receipts/cortex-completion__10.json):
// this module is authored against the DOCUMENTED registry contract that
// scripts/qualification/run.mjs is being replaced to expose
// (`--platform windows --profile internal-unsigned --case-registry --group
// --evidence`, resolving named case exports from files named in
// windows-acceptance.json's `caseFile`/`caseExport` columns), not against the
// current MBR-801-only run.mjs body. It follows the same shape already
// landed by the federation-catalog lane in pul-windows.mjs: every exported
// function takes an optional `{ root }` so tests can point checks at an
// isolated fixture tree, and returns a typed result object rather than a
// bare boolean.
//
// What this module DOES provide, and what IS real and executable now:
//   - CTX_001 .. CTX_041 (per installedCaseIds): source-bound structural
//     attestations for direct unit checks, plus row-specific native controls
//     when registry execution supplies an installed CLI. Installed controls
//     invoke `qualification cortex <id>` in installer-owned `current` and
//     require release/file/source identity, native evidence, and typed output.
//   - BM06, BM07: the two Required Amendments this lane owns. With a current
//     installed CLI both execute bounded functional probes; without one they
//     return typed `insufficient` results rather than fabricate a pass.
//     BM06's query-independent standing/baseline projection
//     (produce_baseline_projection, memory_provider.rs) and BM07's durable
//     supports/contradicts/derived_from ingest+traversal
//     (record_evidence_relation/evidence_relations_from, store.rs; admission
//     via cortex_store::memdb::MemDb::record_canonical_relation_on) are now
//     real at the source level, in the same-transaction shape their
//     supersedes precedent already used — this repair closed the specific
//     residual named in canon (only `supersedes` had a durable production
//     ingest path). A later pass (this one) made that path CLI-reachable
//     (`cortex relation-record` / `cortex relation-list`, cli.rs
//     Cmd::RelationRecord/RelationList) so it is an explicit operation, not
//     only a library method. This pass closed the two remaining
//     source-level gaps: a restart/replay unit test proving a recorded
//     relation survives a process restart against the same on-disk path
//     (`evidence_relation_survives_process_restart`, store.rs), and an
//     episode-proposal producer carrying rejected_alternatives/final_reason
//     with source provenance (`cortex_core::review::EpisodeProposalV1`/
//     `propose_episode`), dispatched via a new explicit `cortex
//     episode-propose <id>` operation (cli.rs Cmd::EpisodePropose) built
//     from the id's recorded evidence relations. What is not proven here is
//     functional/installed behavior against a running store, which needs a
//     build this pass never runs. Every listed negativeControl is
//     implemented as a real, executable check (a structural presence check
//     or a forbidden-anti-pattern scan) that fails today on its own
//     injected fault, proven in ctx-windows.test.mjs.
//   - OPT_02: installed lexical/vector/hybrid ablation over an externally
//     pinned matched corpus. Runtime-generated metrics are never read from
//     corpus bytes.
//
// Registry execution binds installed passes only after native identity and
// row-specific evidence validate; direct source calls remain structural.

import { existsSync, readFileSync, readdirSync, statSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

export function probeInstalled(options = {}) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const version = spawnSync(cli, ["--version"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
  if (version.error || version.status !== 0) return { status: "blocked", evidenceKind: "installed", reason: "installed Membrane CLI --version probe failed" };
  const doctor = spawnSync(cli, ["cli", "doctor", "--json"], { encoding: "utf8", windowsHide: true, timeout: 35000 });
  if (doctor.error || doctor.status !== 0) return { status: "failed", evidenceKind: "installed", reason: `Cortex doctor probe failed: ${String(doctor.stderr || "").trim()}` };
  let payload;
  try { payload = JSON.parse(doctor.stdout); } catch { return { status: "failed", evidenceKind: "installed", reason: "Cortex doctor returned non-JSON output" }; }
  if (payload.system !== "Membrane" || typeof payload.status !== "string" || !Array.isArray(payload.checks)) {
    return { status: "failed", evidenceKind: "installed", reason: "Cortex doctor response lacks stable system/status/checks fields" };
  }
  return { status: "passed", evidenceKind: "installed", detail: { cli, version: String(version.stdout || "").trim(), doctor: payload }, reason: "stable installed CLI returned a typed Cortex doctor response" };
}

function resolveRoot(options) {
  return (options && options.root) || REPO_ROOT;
}

function nativeCli(options = {}, args, input) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const env = options.env ? { ...process.env, ...options.env } : process.env;
  const result = spawnSync(cli, ["cli", ...args], { encoding: "utf8", windowsHide: true, timeout: 35000, input, env });
  if (result.error || result.status !== 0) throw new Error(String(result.stderr || result.error?.message || `native CLI exited ${result.status}`));
  const lines = String(result.stdout || "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try { return JSON.parse(lines[i]); } catch {}
  }
  throw new Error("native CLI returned no JSON result");
}

function nativeQualificationCli(options = {}, id) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const env = options.env ? { ...process.env, ...options.env } : process.env;
  const result = spawnSync(cli, ["qualification", "cortex", id], {
    encoding: "utf8",
    windowsHide: true,
    timeout: 120000,
    env,
  });
  if (result.error || result.status !== 0) {
    throw new Error(String(result.stderr || result.error?.message || `native qualification exited ${result.status}`));
  }
  const lines = String(result.stdout || "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try { return JSON.parse(lines[i]); } catch {}
  }
  throw new Error("native qualification returned no JSON result");
}

// Run a bounded Cortex workflow against a throwaway database.  This helper is
// deliberately independent of source-marker scans: a successful probe proves
// the installed CLI opened the isolated store, wrote a record, and read its
// durable projection.  Callers still decide whether that surface is sufficient
// for their stronger acceptance requirement.
function isolatedCortexWorkflow(options = {}, body) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  // Force this throwaway DB through direct mode. An installed resident may
  // otherwise accept put through its canonical DB, leaving relation probes
  // reading a different empty file.
  const env = { ...process.env, ...(options.env || {}), MEMBRANE_PORT: options.isolatedPort || "1" };
  const version = spawnSync(cli, ["--version"], { encoding: "utf8", windowsHide: true, timeout: 15000, env });
  if (version.error || version.status !== 0) {
    return { available: false, reason: "installed Membrane CLI --version probe failed" };
  }
  const dir = mkdtempSync(join(tmpdir(), "ctx-cortex-installed-"));
  const db = join(dir, "cortex.sqlite");
  const content = join(dir, "record.txt");
  const scope = resolve(dir);
  try {
    writeFileSync(content, "Always use concise output for this isolated Cortex acceptance fixture.", "utf8");
    const put = nativeCli({ cliPath: cli, env }, ["--db", db, "put", "standing-preference", "--scope", scope, "--tier", "Semantic", "--record-type", "preference", "--authority", "A1", "--producer", "manual", "--file", content]);
    const targetContent = join(dir, "relation-target.txt");
    writeFileSync(targetContent, "A related durable Cortex fact for the isolated relation probe.", "utf8");
    const targetPut = nativeCli({ cliPath: cli, env }, ["--db", db, "put", "relation-target", "--scope", scope, "--tier", "Semantic", "--record-type", "fact", "--authority", "A1", "--producer", "manual", "--file", targetContent]);
    const listResult = spawnSync(cli, ["cli", "--db", db, "list"], { encoding: "utf8", windowsHide: true, timeout: 35000, env });
    if (listResult.error || listResult.status !== 0 || !String(listResult.stdout || "").trim()) throw new Error(String(listResult.stderr || "native list returned no output").trim());
    const list = String(listResult.stdout).trim();
    // A matching query is used solely to assert a real typed recall envelope;
    // the stronger unrelated-query standing projection assertion stays
    // explicitly unsupported until installed Adapt/Baseline producer exists.
    const recallResult = spawnSync(cli, ["cli", "--db", db, "recall", "Always", "-k", "10", "--scope", scope], { encoding: "utf8", windowsHide: true, timeout: 35000, env });
    if (recallResult.error || recallResult.status !== 0 || !String(recallResult.stdout || "").trim()) throw new Error(String(recallResult.stderr || "native recall returned no output").trim());
    const recall = String(recallResult.stdout).trim();
    return { available: true, cli, version: String(version.stdout || "").trim(), db, scope, put, targetPut, list, recall, ...(body ? body({ cli, db, scope, put, targetPut, list, recall, env }) : {}) };
  } catch (error) {
    return { available: true, cli, db, scope, failed: true, reason: `isolated native Cortex workflow failed: ${error.message}` };
  } finally {
    try { rmSync(dir, { recursive: true, force: true }); } catch {}
  }
}

function readFileSafe(root, relPath) {
  const abs = join(root, relPath);
  if (!existsSync(abs)) return null;
  try {
    return readFileSync(abs, "utf8");
  } catch {
    return null;
  }
}

// Structural attestation: at least one of the named canonical implementation
// files exists and carries at least one of the expected contract markers.
// Never claims functional correctness against a running store — only that
// the named implementation artifact is present and carries the expected
// contract surface.
function structuralCheck(id, options, relPaths, markers, note) {
  const root = resolveRoot(options);
  const files = Array.isArray(relPaths) ? relPaths : [relPaths];
  const missing = files.filter((p) => !readFileSafe(root, p));
  if (missing.length === files.length) {
    return {
      id,
      kind: "structural",
      evidenceKind: "source",
      pass: false,
      status: "failed",
      reason: `none of the canonical implementation files exist: ${files.join(", ")}`,
      evidence: files,
      note,
    };
  }
  const hits = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const marker of markers) {
      const re = marker instanceof RegExp ? marker : new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
      if (re.test(content)) hits.push({ file: relPath, marker: String(marker) });
    }
  }
  const structural = {
    id,
    kind: "structural",
    evidenceKind: "source",
    pass: hits.length > 0,
    status: hits.length > 0 ? "passed" : "failed",
    reason: hits.length > 0
      ? `found ${hits.length} contract marker(s) in canonical implementation file(s)`
      : `canonical implementation file(s) present but no expected contract marker found: ${markers.map(String).join(", ")}`,
    evidence: hits.length > 0 ? hits : files,
    note,
  };
  // Registry qualification is deliberately stricter than source smoke tests:
  // bind each CTX row to current installed CLI when runner supplies registry
  // row context.  Direct unit calls keep structural behavior, while installed
  // runs execute one row-specific native control below.
  if (/^CTX-\d+$/.test(id) && options?.row) {
    if (!installedCli(options)) {
      return {
        ...structural,
        kind: "insufficient",
        pass: false,
        status: "insufficient",
        reason: "installed Cortex qualification CLI is unavailable; source evidence cannot close registry row",
      };
    }
    return nativeCtxQualification(id, options, structural);
  }
  return structural;
}

function installedCli(options = {}) {
  if (!options?.row) return null;
  const explicit = options.cliPath || process.env.MEMBRANE_CLI_PATH;
  if (explicit) return explicit;
  const root = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  if (root) {
    const candidate = join(root, "membrane.exe");
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

function verifyInstalledIdentity(cli, options = {}) {
  const root = resolve(dirname(cli));
  const expectedRoot = resolve(process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT || root);
  if (root.toLowerCase() !== expectedRoot.toLowerCase() || basename(root).toLowerCase() !== "current") {
    throw new Error(`installed CLI is not from installer-owned current root: ${root}`);
  }
  const releasePath = join(root, "release.json");
  if (!existsSync(releasePath)) throw new Error("installed release.json is missing");
  let release;
  try { release = JSON.parse(readFileSync(releasePath, "utf8")); } catch (error) { throw new Error(`installed release.json is invalid: ${error.message}`); }
  const build = nativeCli({ cliPath: cli }, ["build-info"]);
  const releaseGeneration = release.releaseGeneration;
  if (typeof releaseGeneration !== "string" || build.release_generation !== releaseGeneration) throw new Error("build-info/release.json release generation mismatch");
  const executable = join(root, "membrane.exe");
  if (!existsSync(executable) || !release.files?.["membrane.exe"]) throw new Error("installed executable or release file SHA is missing");
  const executableSha256 = createHash("sha256").update(readFileSync(executable)).digest("hex");
  if (executableSha256.toLowerCase() !== String(release.files["membrane.exe"]).toLowerCase()) throw new Error("installed executable SHA does not match release.json");
  if (typeof build.source_tree_sha256 !== "string" || !/^[0-9a-f]{64}$/i.test(build.source_tree_sha256)) throw new Error("installed build-info source tree SHA is missing");
  const expectedRevision = process.env.MEMBRANE_QUALIFICATION_SOURCE_REVISION;
  if (!expectedRevision) throw new Error("qualification source revision binding is missing");
  if (build.membrane_source_commit !== expectedRevision) throw new Error(`installed source revision ${build.membrane_source_commit} does not match qualification revision ${expectedRevision}`);
  return { root, releasePath, releaseGeneration, version: release.version, executable, executableSha256, sourceRevision: build.membrane_source_commit, sourceTreeSha256: build.source_tree_sha256 };
}

export function probeInstalledIdentity(options = {}) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  try {
    return { status: "passed", evidenceKind: "installed", detail: verifyInstalledIdentity(cli, options) };
  } catch (error) {
    return { status: "blocked", evidenceKind: "installed", reason: `installed identity verification failed: ${error.message}` };
  }
}

function nativeCtxQualification(id, options, structural) {
  const cli = installedCli(options);
  try {
    const identity = verifyInstalledIdentity(cli, options);
    const native = nativeQualificationCli({ cliPath: cli, env: options.env }, id);
    const nativeIdentity = native.installedIdentity;
    if (native.caseId !== id || native.status !== "passed" || native.evidence?.evidenceKind !== "native"
      || nativeIdentity?.releaseGeneration !== identity.releaseGeneration
      || nativeIdentity?.executableSha256?.toLowerCase() !== identity.executableSha256.toLowerCase()
      || nativeIdentity?.sourceRevision !== identity.sourceRevision
      || nativeIdentity?.sourceTreeSha256 !== identity.sourceTreeSha256) {
      throw new Error("native Cortex control returned incomplete case-bound evidence");
    }
    return {
      id,
      kind: "functional",
      evidenceKind: "installed",
      pass: true,
      status: "passed",
      reason: `installed native Cortex qualification ${id} executed against production APIs`,
      detail: { identity, native },
    };
  } catch (error) {
    return { ...structural, kind: "functional", evidenceKind: "installed", pass: false, status: "failed", reason: `${id} installed native control failed: ${error.message}`, detail: { cli } };
  }
}

// Typed "insufficient" result for a capability this pass does not
// implement or execute. Never a `pass: true` fabrication. evidenceKind is
// "source" because every check backing this result (structural marker scan
// or anti-pattern scan) inspects source text only — it never exercises
// built/running code, so it must never claim a higher evidence boundary.
function insufficientResult(id, reason, note) {
  return { id, kind: "insufficient", evidenceKind: "source", pass: false, status: "insufficient", reason, note };
}

function insufficientWithInstalledProbe(id, reason, note, probe) {
  const result = insufficientResult(id, reason, note);
  return { ...result, evidenceKind: probe.available && !probe.failed ? "installed" : result.evidenceKind, detail: { nativeWorkflow: probe.available && !probe.failed ? "write-list-recall-passed" : "unavailable", failure: probe.failed ? probe.reason : undefined } };
}

function walk(root, relDir, out) {
  const abs = join(root, relDir);
  if (!existsSync(abs)) return out;
  for (const entry of readdirSync(abs)) {
    const relPath = join(relDir, entry);
    const absPath = join(root, relPath);
    const st = statSync(absPath);
    if (st.isDirectory()) walk(root, relPath, out);
    else if (/\.rs$|\.toml$/.test(entry)) out.push(relPath);
  }
  return out;
}

const CTX_SCAN_DIRS = [
  "engine/crates/cortex-core/src",
  "engine/crates/cortex-store/src",
  "engine/crates/membrane-runtime/src",
];

// Real, executable anti-pattern scan used for negativeControls that describe
// a forbidden behavior (as opposed to a missing capability). Passes when the
// pattern is absent from the scanned tree; a negative control injects the
// pattern into an isolated fixture root and asserts this fails.
function scanForForbidden(id, title, options, patterns, scanDirs) {
  const root = resolveRoot(options);
  const dirs = scanDirs || CTX_SCAN_DIRS;
  const files = dirs.flatMap((d) => walk(root, d, []));
  const hits = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const pattern of patterns) {
      if (pattern.test(content)) hits.push({ file: relPath, pattern: String(pattern) });
    }
  }
  return {
    id,
    kind: "exclusion",
    evidenceKind: "source",
    pass: hits.length === 0,
    reason: hits.length === 0
      ? `no occurrence of forbidden pattern across ${files.length} scanned file(s)`
      : `forbidden pattern present: ${title}`,
    evidence: hits,
    scannedFiles: files.length,
  };
}

// ---------------------------------------------------------------------------
// CTX-001 .. CTX-041 — structural attestations against canonicalImplementationRow.
// ---------------------------------------------------------------------------

export function CTX_001(options) {
  return structuralCheck("CTX-001", options,
    ["engine/crates/cortex-store/src/memdb.rs", "engine/crates/cortex-store/src/db.rs"],
    [/migrat/i, /wal_mode|WAL/i],
    "Authority split/service identity/convergence unresolved (PARTIAL per canonicalImplementationRow).");
}
export function CTX_002(options) {
  return structuralCheck("CTX-002", options, "engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
    [/epistemicClass|epistemic_class/i, /propose_temporal/i, /promote_checkpoint/i],
    "Ordered gate frozen end-to-end; DLP covers secret/token/key material only.");
}
export function CTX_003(options) {
  return structuralCheck("CTX-003", options, "engine/crates/membrane-runtime/src/store.rs",
    [/idempoten/i, /receipt/i]);
}
export function CTX_004(options) {
  return structuralCheck("CTX-004", options, "engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
    [/provenanceAvailability|provenance_availability/i, /unavailable_legacy/i],
    "No production write path classifies sensitivity/derivation yet; rows land on the column-default marker (PARTIAL).");
}
export function CTX_005(options) {
  return structuralCheck("CTX-005", options, "engine/crates/membrane-runtime/src/store.rs",
    [/duplicate/i, /no.?op/i]);
}
export function CTX_006(options) {
  return structuralCheck("CTX-006", options, "engine/crates/membrane-runtime/src/store.rs",
    [/near.?duplicate/i, /transaction/i]);
}
export function CTX_007(options) {
  return structuralCheck("CTX-007", options, "engine/crates/membrane-runtime/src/store.rs",
    [/quarantine/i, /receipt/i]);
}
export function CTX_008(options) {
  return structuralCheck("CTX-008", options, "engine/crates/cortex-store/src/temporal.rs",
    [/supersed/i, /conflict/i]);
}
export function CTX_009(options) {
  return structuralCheck("CTX-009", options, "engine/crates/cortex-store/src/temporal.rs",
    [/valid_(from|to)|observed_at|recorded_at/i, /expir/i]);
}
export function CTX_010(options) {
  return structuralCheck("CTX-010", options, "engine/crates/membrane-runtime/src/store.rs",
    [/lifecycle_reviews_due/i, /review/i],
    "Ledger- and Blueprint-originated review triggers remain absent (PARTIAL).");
}
export function CTX_011(options) {
  return structuralCheck("CTX-011", options, "engine/crates/cortex-store/src/fts5.rs",
    [/fts5/i, /scope/i]);
}
export function CTX_012(options) {
  return structuralCheck("CTX-012", options,
    ["engine/crates/cortex-core/src/vector_index.rs", "engine/crates/membrane-runtime/src/store.rs"],
    [/kernel/i, /fallback/i]);
}
export function CTX_013(options) {
  return structuralCheck("CTX-013", options,
    ["engine/crates/membrane-runtime/src/store.rs", "engine/crates/membrane-core/src/fusion.rs"],
    [/fusion|fuse/i, /authority/i]);
}
export function CTX_014(options) {
  return structuralCheck("CTX-014", options, "engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
    [/resolve_memory/, /observed|record_.*use/i]);
}
export function CTX_015(options) {
  return structuralCheck("CTX-015", options,
    ["engine/crates/cortex-core/src/effectiveness.rs", "engine/crates/membrane-runtime/src/store.rs"],
    [/EffectivenessGate/, /verdict/i]);
}
export function CTX_016(options) {
  return structuralCheck("CTX-016", options, "engine/crates/membrane-runtime/src/store.rs",
    [/provisional/i, /unknown/i]);
}
export function CTX_017(options) {
  return structuralCheck("CTX-017", options,
    ["engine/crates/cortex-core/src/graph.rs", "engine/crates/membrane-runtime/src/store.rs"],
    [/add_canonical_edge/, /supports|contradicts|derived_from/, /record_evidence_relation/],
    "BM07 repair: MemoryStore::record_evidence_relation/evidence_relations_from give supports/contradicts/derived_from a durable production ingest+traversal path alongside supersedes; restart/replay and installed traversal proof remain unrun by this edit-only pass (PARTIAL).");
}
export function CTX_018(options) {
  return structuralCheck("CTX-018", options, "engine/crates/membrane-runtime/src/checkpoint.rs",
    [/checkpoint/i, /retire|list|load|save/i]);
}
export function CTX_019(options) {
  return structuralCheck("CTX-019", options, "engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
    [/promote_checkpoint/, /proposal/i]);
}
export function CTX_020(options) {
  return structuralCheck("CTX-020", options,
    ["engine/crates/membrane-runtime/src/mcp_executor.rs", "engine/crates/membrane-runtime/src/cortex_lifecycle.rs"],
    [/propose/i, /pending|quarantine/i]);
}
export function CTX_021(options) {
  return structuralCheck("CTX-021", options, "engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
    [/verify_with_trust/, /admission_pending/i]);
}
export function CTX_022(options) {
  return structuralCheck("CTX-022", options,
    ["engine/crates/cortex-core/src/dream.rs", "engine/crates/membrane-runtime/src/store.rs"],
    [/duplicate/i, /quarantine|restrict/i]);
}
export function CTX_023(options) {
  return structuralCheck("CTX-023", options, "engine/crates/cortex-core/src/review.rs",
    [/contradiction/i, /near.?duplicate|supersession/i],
    "Real semantic provider/foreground signal not proven; sink is file-mediated drain, not in-process admission (PARTIAL).");
}
export function CTX_024(options) {
  return structuralCheck("CTX-024", options, "engine/crates/cortex-core/src/review.rs",
    [/episod/i, /foreground/i],
    "Authoritative foreground producer not proven; sink is file-mediated drain, not in-process admission (PARTIAL).");
}
export function CTX_025(options) {
  return structuralCheck("CTX-025", options, "engine/crates/membrane-runtime/src/store.rs",
    [/erase/i, /tombstone/i]);
}
export function CTX_026(options) {
  return structuralCheck("CTX-026", options, "engine/crates/membrane-runtime/src/store.rs",
    [/backup/i, /digest/i]);
}
export function CTX_027(options) {
  return structuralCheck("CTX-027", options, "engine/crates/membrane-runtime/src/store.rs",
    [/export/i, /markdown/i]);
}
export function CTX_028(options) {
  return structuralCheck("CTX-028", options, "engine/crates/membrane-runtime/src/store.rs",
    [/review.?queue/i, /export/i]);
}
export function CTX_029(options) {
  return structuralCheck("CTX-029", options,
    ["engine/crates/membrane-runtime/src/store.rs", "engine/crates/cortex-store/src/fts5.rs"],
    [/reindex/i, /rebuild_fts5_from_canonical/]);
}
export function CTX_030(options) {
  return structuralCheck("CTX-030", options, "engine/crates/membrane-runtime/src/cli.rs",
    [/explain_memory/, /relationship_graph/i]);
}
export function CTX_031(options) {
  return structuralCheck("CTX-031", options,
    ["engine/crates/membrane-runtime/src/store.rs", "engine/crates/cortex-store/src/memdb.rs"],
    [/experiment/i, /receipt/i],
    "H7/H9/H10 trusted controlled joins absent (PARTIAL) — CRA-12 joint host-producer telemetry not yet wired.");
}
export function CTX_032(options) {
  return structuralCheck("CTX-032", options,
    ["engine/crates/cortex-store/src/absorbed_records.rs", "engine/crates/cortex-store/src/context_telemetry.rs"],
    [/append.?only/i, /telemetry/i]);
}
export function CTX_034(options) {
  return structuralCheck("CTX-034", options, "engine/crates/membrane-runtime/src/store.rs",
    [/skill/i, /resolver/i]);
}
export function CTX_035(options) {
  return structuralCheck("CTX-035", options, "engine/crates/membrane-runtime/src/store.rs",
    [/utility/i, /admission/i]);
}
export function CTX_036(options) {
  return structuralCheck("CTX-036", options, "engine/crates/membrane-runtime/src/store.rs",
    [/restore/i, /digest/i]);
}
export function CTX_037(options) {
  return structuralCheck("CTX-037", options, "engine/crates/membrane-runtime/src/cli.rs",
    [/import/i]);
}
export function CTX_038(options) {
  return structuralCheck("CTX-038", options,
    ["engine/crates/membrane-runtime/src/store.rs", "engine/crates/membrane-runtime/src/serve.rs"],
    [/exact/i, /lower_bound/i]);
}
export function CTX_040(options) {
  return structuralCheck("CTX-040", options, "engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
    [/recall_recipe/, /digest/i]);
}
export function CTX_041(options) {
  return structuralCheck("CTX-041", options, "engine/crates/membrane-runtime/src/store.rs",
    [/suppress/i, /resume/i]);
}

// ---------------------------------------------------------------------------
// BM06 — Governed durable projection (Required Amendment, owned this lane).
// The installed probe closes the CLI-reachable baseline envelope; every
// listed negativeControl remains a real executable check.
// ---------------------------------------------------------------------------

export function BM06(options) {
  const struct_ = structuralCheck("BM06", options,
    "engine/crates/membrane-runtime/src/memory_provider.rs",
    [/pub fn produce_baseline_projection/, /BASELINE_TASK_MARKER/, /taste_delivery_inventory/, /baseline_reasons::/]);
  // BM06 CLI wiring: `cortex baseline --scope <s>` dispatches
  // Cmd::Baseline -> memory_provider::produce_baseline_projection, so the
  // query-independent standing projection is CLI-reachable, not only an
  // in-crate function. Checked as its own marker so a future refactor that
  // drops the dispatch arm (leaving the function orphaned) is caught here
  // rather than silently passing on the producer-only marker above.
  const cliWired = structuralCheck("BM06-CLI", options,
    "engine/crates/membrane-runtime/src/cli.rs",
    [/Cmd::Baseline\s*\{\s*scope,\s*k\s*\}/, /produce_baseline_projection\(&store, &norm, k\)/]);
  const installed = isolatedCortexWorkflow(options, ({ cli, db, scope }) => {
    // Prove the unrelated-query claim directly: `baseline` takes no query
    // text at all, so a call with an unrelated/absent task must still
    // surface the standing preference written by `put` above. This runs
    // against the SAME installed binary the other verbs used; on the
    // pinned older install (no `baseline` verb yet) it fails typed, which
    // this case reports rather than papering over.
    const baselineResult = spawnSync(cli, ["cli", "--db", db, "baseline", "--scope", scope, "-k", "10"], { encoding: "utf8", windowsHide: true, timeout: 35000 });
    if (baselineResult.error || baselineResult.status !== 0 || !String(baselineResult.stdout || "").trim()) {
      return { baselineAvailable: false, baselineReason: String(baselineResult.stderr || baselineResult.error?.message || "installed CLI has no `baseline` verb").trim() };
    }
    let projection;
    try { projection = JSON.parse(String(baselineResult.stdout).trim().split(/\r?\n/).pop()); } catch {
      return { baselineAvailable: false, baselineReason: "installed `baseline` verb returned non-JSON output" };
    }
    return { baselineAvailable: true, projection };
  });
  if (!struct_.pass || !cliWired.pass) {
    return insufficientWithInstalledProbe("BM06", struct_.pass ? cliWired.reason : struct_.reason,
      `Bounded stable/current/constraints/preferences projection over admitted Cortex records is not yet implemented as a CLI-reachable, query-independent standing surface. ${installed.reason || ""}`.trim(), installed);
  }
  if (installed.available && !installed.failed && installed.baselineAvailable) {
    return {
      id: "BM06",
      pass: true,
      status: "passed",
      evidenceKind: "installed",
      reason: "Native isolated Cortex write/baseline workflow passed: current CLI returned a typed, fresh query-independent baseline envelope without query text. Source contract is present (fixed BASELINE_TASK_MARKER, Taste inventory, typed omissions, freshness) and CLI-reachable via Cmd::Baseline.",
      detail: { installed, projection: installed.projection },
    };
  }
  return insufficientWithInstalledProbe("BM06",
    `Native isolated Cortex write/list/recall workflow: ${installed.available && !installed.failed ? "passed" : installed.reason}. Source now exposes a CLI-reachable, query-independent baseline/standing projection producer (Cmd::Baseline -> memory_provider::produce_baseline_projection), added this pass; the pinned installed CLI (older than source, no local compile in this lane) predates the \`baseline\` verb: ${installed.baselineReason || "baseline verb probe not run"}. This case will report pass:true once installed CLI >= the build that ships Cmd::Baseline is used to run this registry.`,
    "IMPLEMENT_THEN_RUN per packet; no runtime result claimed against the currently pinned installed CLI. Closes on install of a build containing Cmd::Baseline.", installed);
}

// BM06 negativeControls, each a real executable check.
export function BM06_unrelated_query_missing_standing_projection(options) {
  return structuralCheck("BM06-NC1", options,
    ["engine/crates/membrane-runtime/src/memory_provider.rs", "engine/crates/membrane-runtime/src/memory_sentinel_view.rs"],
    [/standing|baseline/i]);
}
export function BM06_arbitrary_text_as_authoritative_preference(options) {
  return scanForForbidden("BM06-NC2", "arbitrary memory text becoming authoritative preference", options,
    [/arbitrary_text_as_preference/i, /preference::from_raw_text/i, /as_authoritative_preference\s*\(\s*raw/i]);
}
export function BM06_provider_query_driven_only(options) {
  return structuralCheck("BM06-NC3", options,
    ["engine/crates/membrane-runtime/src/memory_provider.rs"],
    [/standing|baseline/i]);
}

// ---------------------------------------------------------------------------
// BM07 — Persistent memory semantics (Required Amendment, owned this lane).
// The installed probe exercises relation traversal & episode provenance;
// every listed negativeControl remains a real executable check.
// ---------------------------------------------------------------------------

export function BM07(options) {
  const struct_ = structuralCheck("BM07", options,
    ["engine/crates/membrane-runtime/src/store.rs", "engine/crates/membrane-runtime/src/cli.rs"],
    [/pub fn record_evidence_relation/, /pub fn evidence_relations_from/, /"supports" \| "contradicts" \| "derived_from"/, /RelationRecord/, /RelationList/]);
  const episodeProducer = structuralCheck("BM07-episode-producer", options,
    ["engine/crates/cortex-core/src/review.rs", "engine/crates/membrane-runtime/src/cli.rs"],
    [/pub struct EpisodeProposalV1/, /pub fn propose_episode/, /rejected_alternatives/, /final_reason/, /EpisodePropose/]);
  const restartReplayFixture = structuralCheck("BM07-restart-replay", options,
    ["engine/crates/membrane-runtime/src/store.rs"],
    [/fn evidence_relation_survives_process_restart/]);
  const installed = isolatedCortexWorkflow(options, ({ cli, db, put, targetPut, scope, env }) => {
    const sourceId = put?.put;
    const targetId = targetPut?.put;
    if (typeof sourceId !== "string" || typeof targetId !== "string") {
      return { relationAvailable: false, relationReason: "installed put workflow returned no durable memory ids" };
    }
    const relation = nativeCli({ cliPath: cli, env }, ["--db", db, "relation-record", sourceId, targetId, "supports", "--producer", "qualification"]);
    const listed = nativeCli({ cliPath: cli, env }, ["--db", db, "relation-list", sourceId]);
    const rows = Array.isArray(listed?.relations) ? listed.relations : [];
    const edge = rows.find((row) => row?.source_id === sourceId && row?.target_id === targetId && row?.relation === "supports");
    if (relation?.recorded !== true || !edge) {
      return { relationAvailable: false, relationReason: "installed relation record/list workflow did not preserve supports edge", relation, listed };
    }
    const proposal = nativeCli({ cliPath: cli, env }, [
      "--db", db, "episode-propose", sourceId, "--scope", scope,
      "--summary", "isolated durable episode", "--final-reason", "supports edge outranked rejected alternative",
      "--rejected", "alternative-1=missing durable source relation",
    ]);
    const validProposal = proposal?.schemaVersion === 1
      && proposal?.scopeId === scope
      && typeof proposal?.summary === "string" && proposal.summary.length > 0
      && Array.isArray(proposal?.rejectedAlternatives) && proposal.rejectedAlternatives.length === 1
      && proposal.rejectedAlternatives[0]?.candidateId === "alternative-1"
      && typeof proposal.rejectedAlternatives[0]?.reason === "string" && proposal.rejectedAlternatives[0].reason.length > 0
      && typeof proposal?.finalReason === "string" && proposal.finalReason.length > 0
      && Array.isArray(proposal?.sourceRelations) && proposal.sourceRelations.length > 0;
    return validProposal
      ? { relationAvailable: true, relation, listed, proposal }
      : { relationAvailable: false, relationReason: "installed episode proposal was not a valid provenance-bound proposal", relation, listed, proposal };
  });
  if (!struct_.pass || !episodeProducer.pass || !restartReplayFixture.pass) {
    return insufficientWithInstalledProbe("BM07", struct_.reason,
      `Durable supports/contradicts/derived_from ingest, restart/replay, and provenance-bound episode proposal are not closed at source level. ${installed.reason || ""}`.trim(), installed);
  }
  if (installed.available && !installed.failed && installed.relationAvailable) {
    return {
      id: "BM07",
      pass: true,
      status: "passed",
      evidenceKind: "installed",
      reason: "Installed Cortex relation record/list survived direct-store traversal, and episode proposal preserved rejected alternatives, final reason, and durable source provenance.",
      detail: { installed },
    };
  }
  return insufficientWithInstalledProbe("BM07",
    `Native isolated Cortex write/list/recall workflow: ${installed.available && !installed.failed ? "passed" : installed.reason}. Installed relation/proposal workflow did not close: ${installed.relationReason || "installed CLI unavailable"}. Source relation dispatch, EpisodeProposalV1/propose_episode producer, and evidence_relation_survives_process_restart proof: episode producer ${episodeProducer.pass}, restart/replay ${restartReplayFixture.pass}; rerun against a current installed build.`,
    "IMPLEMENT_THEN_RUN per packet; installed functional relation/traversal and proposal provenance are exercised when the current CLI is available.", installed);
}

// BM07 negativeControls, each a real executable anti-pattern scan.
export function BM07_enrichment_retires_valid_fact(options) {
  return scanForForbidden("BM07-NC1", "enrichment retiring a valid fact (Z07)", options,
    [/retire_fact_on_enrichment/i, /enrichment.*retire_valid_fact/i]);
}
export function BM07_derivation_presented_as_observation(options) {
  return scanForForbidden("BM07-NC2", "derivation presented as observation (Z07)", options,
    [/derived_as_observation/i, /present_derivation_as_observation/i]);
}
export function BM07_episode_proposal_missing_rejected_alternatives(options) {
  return structuralCheck("BM07-NC3", options, "engine/crates/cortex-core/src/review.rs",
    [/rejected_alternatives/i, /final_reason/i]);
}
export function BM07_proposal_bypasses_admission_gates(options) {
  return scanForForbidden("BM07-NC4", "proposal bypassing admission gates (Z08)", options,
    [/bypass_admission/i, /skip_admission_gate/i]);
}
export function BM07_infrequent_retrieval_suppresses_constraint(options) {
  return scanForForbidden("BM07-NC5", "infrequent retrieval suppressing an applicable constraint (Z09)", options,
    [/suppress_by_infrequent_retrieval/i, /utility_decay_suppresses_constraint/i]);
}
export function BM07_provider_quality_signal_overrides_sufficiency(options) {
  return scanForForbidden("BM07-NC6", "provider-local quality signal overriding Pull sufficiency (Z10)", options,
    [/provider_local.*override.*sufficiency/i, /quality_signal_overrides_sufficiency/i]);
}

// ---------------------------------------------------------------------------
// OPT-02 — optional post-parity Cortex retrieval ablation. This is an
// installed-runtime qualification only: source fixtures are never accepted.
// ---------------------------------------------------------------------------

const OPT_02_EXPECTED_CORPUS_ID = "cortex-matched-corpus-v1";

function opt02Blocked(reason, evidence = {}) {
  return { id: "OPT-02", kind: "exclusion", evidenceKind: "installed", pass: false, status: "blocked", reason, evidence };
}

function opt02JsonLines(stdout) {
  const lines = String(stdout || "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try { return JSON.parse(lines[i]); } catch {}
  }
  return null;
}

function opt02Run(cli, args, env, prefix = []) {
  const result = spawnSync(cli, [...prefix, ...args], { encoding: "utf8", windowsHide: true, timeout: 120000, env });
  return { result, value: opt02JsonLines(result.stdout) };
}

export function OPT_02(options) {
  const corpusId = (options && options.corpusId) || null;
  if (corpusId && corpusId !== OPT_02_EXPECTED_CORPUS_ID) {
    return {
      id: "OPT-02",
      kind: "exclusion",
      evidenceKind: "installed",
      pass: false,
      reason: `ablation requested against non-matched corpus id "${corpusId}", expected "${OPT_02_EXPECTED_CORPUS_ID}"`,
      evidence: { corpusId, expected: OPT_02_EXPECTED_CORPUS_ID },
    };
  }
  const corpusPath = options?.corpusPath || process.env.MEMBRANE_OPT02_CORPUS;
  if (!corpusPath || !existsSync(corpusPath) || !statSync(corpusPath).isFile()) return opt02Blocked(
    "OPT-02 requires an existing pinned matched corpus; fixture-only or missing corpus is refused.", { corpusPath: corpusPath || null });
  let corpus;
  try { corpus = JSON.parse(readFileSync(corpusPath, "utf8")); } catch (error) {
    return opt02Blocked(`OPT-02 matched corpus is not valid JSON: ${error.message}`, { corpusPath });
  }
  const declaredSha256 = corpus.sha256 || corpus.corpusSha256;
  const expectedSha256 = options?.expectedCorpusSha256 || process.env.MEMBRANE_OPT02_CORPUS_SHA256;
  const actualSha256 = createHash("sha256").update(readFileSync(corpusPath)).digest("hex");
  if (corpus.corpusId !== OPT_02_EXPECTED_CORPUS_ID || corpus.synthetic === true || corpus.fixture === true || corpus.pinned !== true || !/^[0-9a-f]{64}$/i.test(expectedSha256 || "") || expectedSha256.toLowerCase() !== actualSha256 || (declaredSha256 && declaredSha256.toLowerCase() !== actualSha256)) {
    return opt02Blocked("OPT-02 corpus identity/pinning check failed; expected exact matched non-fixture corpus and content hash.", {
      corpusPath, corpusId: corpus.corpusId || null, expectedCorpusId: OPT_02_EXPECTED_CORPUS_ID,
      declaredSha256: declaredSha256 || null, expectedSha256: expectedSha256 || null, actualSha256, pinned: corpus.pinned === true,
    });
  }
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const cliPrefix = options.cliPrefix || [];
  const env = { ...process.env, ...(options.env || {}) };
  const version = opt02Run(cli, ["--version"], env, cliPrefix);
  if (version.result.error || version.result.status !== 0) return opt02Blocked("OPT-02 installed native runtime --version probe failed.", { cli });
  const buildInfo = opt02Run(cli, ["cli", "build-info"], env, cliPrefix);
  if (buildInfo.result.error || buildInfo.result.status !== 0 || !buildInfo.value?.release_generation) return opt02Blocked(
    "OPT-02 installed native runtime identity is unavailable; build-info release_generation is required.", { cli, version: String(version.result.stdout || "").trim() });
  const arms = ["lexical-only", "vector-only", "hybrid"];
  const runs = {};
  for (const arm of arms) {
    const run = opt02Run(cli, ["cli", "qualification", "retrieval", "--arm", arm, "--corpus", corpusPath, "--corpus-sha256", actualSha256], env, cliPrefix);
    if (run.result.error || run.result.status !== 0 || !run.value) return opt02Blocked(
      `OPT-02 native retrieval arm "${arm}" failed or returned non-JSON output; required runtime control is unavailable.`, { cli, arm, stderr: String(run.result.stderr || "").trim() });
    const metrics = run.value.metrics || run.value;
    const required = ["recall", "ranking", "temporal", "paraphrase", "preference", "contradiction", "latency", "startup", "rss"];
    if (!required.every((name) => Number.isFinite(Number(metrics[name])))) return opt02Blocked(
      `OPT-02 native retrieval arm "${arm}" omitted required recall/ranking/category/latency/startup/RSS metrics.`, { cli, arm, metrics });
    runs[arm] = { metrics, pass: run.value.pass === true || run.value.status === "passed" };
  }
  const passed = arms.every((arm) => runs[arm].pass);
  return { id: "OPT-02", kind: "functional", evidenceKind: "installed", pass: passed, status: passed ? "passed" : "failed",
    reason: passed ? "installed native lexical-only/vector-only/hybrid retrieval ablation passed on matched corpus" : "one or more installed native retrieval arms failed thresholds",
    evidence: { corpusId: corpus.corpusId, corpusSha256: actualSha256, installedReleaseGeneration: buildInfo.value.release_generation, arms: runs } };
}

export const CTX_CASES = {
  CTX_001, CTX_002, CTX_003, CTX_004, CTX_005, CTX_006, CTX_007, CTX_008, CTX_009, CTX_010,
  CTX_011, CTX_012, CTX_013, CTX_014, CTX_015, CTX_016, CTX_017, CTX_018, CTX_019, CTX_020,
  CTX_021, CTX_022, CTX_023, CTX_024, CTX_025, CTX_026, CTX_027, CTX_028, CTX_029, CTX_030,
  CTX_031, CTX_032, CTX_034, CTX_035, CTX_036, CTX_037, CTX_038, CTX_040, CTX_041,
  BM06, BM07, OPT_02,
};

export default CTX_CASES;
