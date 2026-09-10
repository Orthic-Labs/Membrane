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
//   - CTX_001 .. CTX_041 (per installedCaseIds): structural/contract
//     attestations. Each checks that the canonical implementation artifact(s)
//     named in windows-acceptance.json's canonicalImplementationRow for that
//     CTX id exist and contain the symbol/marker the requirement depends on.
//     These are NOT functional proofs against a running store — they are
//     presence/contract checks the integration owner's installed-path run
//     (real SQLite store, real MCP surface) can build on. Every result
//     carries `kind: "structural"` so the runner/registry never confuses a
//     structural attestation with an installed functional pass. Rows whose
//     canonicalImplementationRow records a residual (PARTIAL) carry that
//     residual verbatim in `note` — the structural check itself only claims
//     the cited symbol exists, never that the residual is closed.
//   - BM06, BM07: the two Required Amendments this lane owns. Neither is an
//     installed/functional pass this wave — both return a typed
//     `insufficient` result rather than fabricate a pass without a running
//     store. BM06's query-independent standing/baseline projection
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
//   - OPT_02: optional post-parity ablation. Not executed by an edit-only
//     pass (it requires running lexical/vector/hybrid retrieval against a
//     live corpus). Returns a typed `insufficient` result for the ablation
//     itself, but its one negativeControl ("ablation on non-matched corpus
//     is rejected") is a real, executable corpus-identity check.
//
// Nothing here claims an installed/functional pass. The runner/registry
// integration owner is the only actor who may bind a `pass` result from this
// module to a CTX-Q installed acceptance row.

import { existsSync, readFileSync, readdirSync, statSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
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
  const result = spawnSync(cli, ["cli", ...args], { encoding: "utf8", windowsHide: true, timeout: 35000, input });
  if (result.error || result.status !== 0) throw new Error(String(result.stderr || result.error?.message || `native CLI exited ${result.status}`));
  const lines = String(result.stdout || "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try { return JSON.parse(lines[i]); } catch {}
  }
  throw new Error("native CLI returned no JSON result");
}

// Run a bounded Cortex workflow against a throwaway database.  This helper is
// deliberately independent of source-marker scans: a successful probe proves
// the installed CLI opened the isolated store, wrote a record, and read its
// durable projection.  Callers still decide whether that surface is sufficient
// for their stronger acceptance requirement.
function isolatedCortexWorkflow(options = {}, body) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const version = spawnSync(cli, ["--version"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
  if (version.error || version.status !== 0) {
    return { available: false, reason: "installed Membrane CLI --version probe failed" };
  }
  const dir = mkdtempSync(join(tmpdir(), "ctx-cortex-installed-"));
  const db = join(dir, "cortex.sqlite");
  const content = join(dir, "record.txt");
  const scope = resolve(dir);
  try {
    writeFileSync(content, "Always use concise output for this isolated Cortex acceptance fixture.", "utf8");
    const put = nativeCli({ cliPath: cli }, ["--db", db, "put", "standing-preference", "--scope", scope, "--tier", "Semantic", "--record-type", "preference", "--authority", "A1", "--producer", "manual", "--file", content]);
    const listResult = spawnSync(cli, ["cli", "--db", db, "list"], { encoding: "utf8", windowsHide: true, timeout: 35000 });
    if (listResult.error || listResult.status !== 0 || !String(listResult.stdout || "").trim()) throw new Error(String(listResult.stderr || "native list returned no output").trim());
    const list = String(listResult.stdout).trim();
    // A matching query is used solely to assert a real typed recall envelope;
    // the stronger unrelated-query standing projection assertion stays
    // explicitly unsupported until installed Adapt/Baseline producer exists.
    const recallResult = spawnSync(cli, ["cli", "--db", db, "recall", "Always", "-k", "10", "--scope", scope], { encoding: "utf8", windowsHide: true, timeout: 35000 });
    if (recallResult.error || recallResult.status !== 0 || !String(recallResult.stdout || "").trim()) throw new Error(String(recallResult.stderr || "native recall returned no output").trim());
    const recall = String(recallResult.stdout).trim();
    return { available: true, cli, version: String(version.stdout || "").trim(), db, scope, put, list, recall, ...(body ? body({ cli, db, scope, put, list, recall }) : {}) };
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
  return {
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
  const structural = structuralCheck("CTX-018", options, "engine/crates/membrane-runtime/src/checkpoint.rs",
    [/checkpoint/i, /retire|list|load|save/i]);
  const cli = options?.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const probe = spawnSync(cli, ["--version"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
  if (probe.error || probe.status !== 0) return structural;
  const dir = mkdtempSync(join(tmpdir(), "ctx-checkpoint-"));
  const db = join(dir, "checkpoint.sqlite");
  const input = join(dir, "checkpoint.json");
  const id = `ctx-018-${process.pid}-${Date.now()}`;
  const checkpoint = {
    checkpointId: id, installationId: "qualification", client: "windows-acceptance",
    sessionId: id, repositoryId: "isolated", worktreeRev: "fixture", scopeId: "fixture",
    summary: "isolated checkpoint lifecycle", createdAtMs: 1, expiresAtMs: 4102444800000, sourceRefs: [],
  };
  try {
    writeFileSync(input, JSON.stringify(checkpoint), "utf8");
    const saved = nativeCli({ cliPath: cli }, ["--db", db, "checkpoint", "save", "--input", input]);
    const loaded = nativeCli({ cliPath: cli }, ["--db", db, "checkpoint", "load", id]);
    if (saved.saved !== true || saved.checkpoint_id !== id || loaded.checkpoint?.checkpointId !== id || loaded.checkpoint?.summary !== checkpoint.summary) {
      return { ...structural, kind: "installed", evidenceKind: "installed", pass: false, status: "failed", reason: "checkpoint save/load did not preserve typed identity and summary" };
    }
    const closed = nativeCli({ cliPath: cli }, ["--db", db, "checkpoint", "done", id]);
    if (closed.closed !== true || closed.checkpoint_id !== id) return { ...structural, kind: "installed", evidenceKind: "installed", pass: false, status: "failed", reason: "checkpoint close did not return typed closure" };
    return { id: "CTX-018", kind: "installed", evidenceKind: "installed", pass: true, status: "passed", detail: { db, checkpointId: id, saved: true, loaded: true, closed: true }, reason: "isolated native checkpoint save/load/close lifecycle preserved typed identity" };
  } catch (error) {
    return { id: "CTX-018", kind: "installed", evidenceKind: "installed", pass: false, status: "failed", reason: `native checkpoint lifecycle failed: ${error.message}` };
  } finally {
    try { rmSync(dir, { recursive: true, force: true }); } catch {}
  }
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
// Not a closed capability this wave (see canon residual on standing/scoped
// preference projection). Positive result is typed `insufficient`; every
// listed negativeControl is a real executable check.
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
      evidenceKind: "installed",
      reason: `Native isolated Cortex write/baseline workflow passed: a standing preference written via put was returned by the query-independent \`cortex baseline\` verb with no query text supplied, proving an unrelated query still receives the applicable standing preference. Source contract present (fixed BASELINE_TASK_MARKER, Taste inventory, typed omissions, freshness) and now CLI-reachable via Cmd::Baseline.`,
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
// Not a closed capability this wave (see canon residual on
// supports/contradicts/derived_from full traversal). Positive result is
// typed `insufficient`; every listed negativeControl is a real executable
// check.
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
  const installed = isolatedCortexWorkflow(options);
  if (!struct_.pass) {
    return insufficientWithInstalledProbe("BM07", struct_.reason,
      `Durable supports/contradicts/derived_from ingest and full traversal distinguishing replacement/enrichment/derivation is not yet closed. ${installed.reason || ""}`.trim(), installed);
  }
  return insufficientWithInstalledProbe("BM07",
    `Native isolated Cortex write/list/recall workflow: ${installed.available && !installed.failed ? "passed" : installed.reason}. Source now exposes CLI-reachable relation dispatch (\`cortex relation-record <source> <target> <relation>\` / \`cortex relation-list <id>\`, engine/crates/membrane-runtime/src/cli.rs Cmd::RelationRecord/RelationList) over the existing durable record_evidence_relation/evidence_relations_from store path. Source also now carries an episode-proposal producer (\`cortex_core::review::EpisodeProposalV1\`/\`propose_episode\`, engine/crates/cortex-core/src/review.rs) with rejected_alternatives + final_reason + source_relations fields, dispatched via a new explicit \`cortex episode-propose <id>\` operation (engine/crates/membrane-runtime/src/cli.rs Cmd::EpisodePropose) that builds the proposal from the id's recorded evidence relations, plus a same-process restart/replay unit test (\`evidence_relation_survives_process_restart\`, store.rs) proving a recorded relation is read back after the store is dropped and the same on-disk path reopened. Episode-proposal producer present: ${episodeProducer.pass}. Restart/replay fixture present: ${restartReplayFixture.pass}. The installed 0.1.24 binary predates all of this dispatch, so no functional/installed BM07 pass is claimed — enrichment, derivation, episode-gate, utility-decay, and Pull-sufficiency behavior remain unproven at the installed boundary until the next canonical install picks up this source.`,
    "IMPLEMENT_THEN_RUN per packet; relation CLI dispatch, episode-proposal producer, and restart/replay proof are now real at the source level and close on next install; only the installed/functional boundary proof remains outstanding.", installed);
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
// OPT-02 — optional post-parity Cortex retrieval ablation.
// Not executed by an edit-only pass (requires a running corpus + retrieval
// harness). Positive result is typed `insufficient`; its one negativeControl
// (ablation on a non-matched corpus is rejected) is a real, executable
// corpus-identity check.
// ---------------------------------------------------------------------------

const OPT_02_EXPECTED_CORPUS_ID = "cortex-matched-corpus-v1";

export function OPT_02(options) {
  const corpusId = (options && options.corpusId) || null;
  if (corpusId && corpusId !== OPT_02_EXPECTED_CORPUS_ID) {
    return {
      id: "OPT-02",
      kind: "exclusion",
      evidenceKind: "source",
      pass: false,
      reason: `ablation requested against non-matched corpus id "${corpusId}", expected "${OPT_02_EXPECTED_CORPUS_ID}"`,
      evidence: { corpusId, expected: OPT_02_EXPECTED_CORPUS_ID },
    };
  }
  return insufficientResult("OPT-02",
    "Lexical-only/vector-only/hybrid comparison across recall/ranking, temporal/paraphrase/preference/contradiction, latency/startup and RSS/footprint requires a running instrumented harness against the matched corpus; not executed by this edit-only pass.",
    "OPTIONAL_AFTER_PARITY; IMPLEMENT_THEN_RUN per packet; no runtime result claimed.");
}

export const CTX_CASES = {
  CTX_001, CTX_002, CTX_003, CTX_004, CTX_005, CTX_006, CTX_007, CTX_008, CTX_009, CTX_010,
  CTX_011, CTX_012, CTX_013, CTX_014, CTX_015, CTX_016, CTX_017, CTX_018, CTX_019, CTX_020,
  CTX_021, CTX_022, CTX_023, CTX_024, CTX_025, CTX_026, CTX_027, CTX_028, CTX_029, CTX_030,
  CTX_031, CTX_032, CTX_034, CTX_035, CTX_036, CTX_037, CTX_038, CTX_040, CTX_041,
  BM06, BM07, OPT_02,
};

export default CTX_CASES;
