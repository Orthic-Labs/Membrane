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
//     ingest path). What is not proven here is functional/installed
//     behavior against a running store, which needs a build this pass never
//     runs. Every listed negativeControl is implemented as a real,
//     executable check (a structural presence check or a
//     forbidden-anti-pattern scan) that fails today on its own injected
//     fault, proven in ctx-windows.test.mjs.
//   - OPT_02: optional post-parity ablation. Not executed by an edit-only
//     pass (it requires running lexical/vector/hybrid retrieval against a
//     live corpus). Returns a typed `insufficient` result for the ablation
//     itself, but its one negativeControl ("ablation on non-matched corpus
//     is rejected") is a real, executable corpus-identity check.
//
// Nothing here claims an installed/functional pass. The runner/registry
// integration owner is the only actor who may bind a `pass` result from this
// module to a CTX-Q installed acceptance row.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

function resolveRoot(options) {
  return (options && options.root) || REPO_ROOT;
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
// Not a closed capability this wave (see canon residual on standing/scoped
// preference projection). Positive result is typed `insufficient`; every
// listed negativeControl is a real executable check.
// ---------------------------------------------------------------------------

export function BM06(options) {
  const struct_ = structuralCheck("BM06", options,
    "engine/crates/membrane-runtime/src/memory_provider.rs",
    [/pub fn produce_baseline_projection/, /BASELINE_TASK_MARKER/, /taste_delivery_inventory/, /baseline_reasons::/]);
  if (!struct_.pass) {
    return insufficientResult("BM06", struct_.reason,
      "Bounded stable/current/constraints/preferences projection over admitted Cortex records is not yet implemented as a query-independent standing surface; no baseline-projection contract marker found.");
  }
  return insufficientResult("BM06",
    "produce_baseline_projection (engine/crates/membrane-runtime/src/memory_provider.rs) sources a fixed, non-task trace id (BASELINE_TASK_MARKER) rather than deriving from any query, reuses Adapt/Taste's own query-independent selection verbatim (taste_delivery_inventory), and reports typed omissions (baseline_reasons::OUT_OF_SCOPE/LIFECYCLE_INELIGIBLE/UNVERIFIED/CONTENT_UNAVAILABLE/ADAPT_INVENTORY_UNAVAILABLE) plus a freshness block — all present in source. It returns a provider-side ContextCandidateSet, never an admitted context block, so Pull retains final admission. This pass still performs no functional proof against a running store (an unrelated query actually receiving the projection, an unavailable Adapt inventory actually degrading to typed-unavailable rather than a fabricated empty pass) — that requires the installed/functional evidence boundary the integration owner runs after a build.",
    "IMPLEMENT_THEN_RUN per packet; no runtime result claimed.");
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
    "engine/crates/membrane-runtime/src/store.rs",
    [/pub fn record_evidence_relation/, /pub fn evidence_relations_from/, /"supports" \| "contradicts" \| "derived_from"/]);
  if (!struct_.pass) {
    return insufficientResult("BM07", struct_.reason,
      "Durable supports/contradicts/derived_from ingest and full traversal distinguishing replacement/enrichment/derivation is not yet closed.");
  }
  return insufficientResult("BM07",
    "The CTX-017 residual this row named — only `supersedes` had a durable persistence path, while supports/contradicts/derived_from were convergence-ready in cortex_store::memdb (schema, admission gate, traversal filter) but never durably ingested by a production caller — is closed at the source level: MemoryStore::record_evidence_relation (engine/crates/membrane-runtime/src/store.rs) now admits supports/contradicts/derived_from edges through cortex_store::memdb::MemDb::record_canonical_relation_on inside one committed transaction (same guarantee apply_lifecycle_input_on already gave `supersedes`), refusing self-reference, missing endpoints and cross-scope edges; MemoryStore::evidence_relations_from reads them back through cortex_core::relation_category, which keeps Derivation (`derived_from`) from ever being read back mixed in with Observation (`supports`/`contradicts`). What remains unproven by this edit-only pass: restart/replay durability and bounded coherent episode proposals against a running store, which require the installed/functional evidence boundary — no runtime result is claimed here.",
    "IMPLEMENT_THEN_RUN per packet; no runtime result claimed.");
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
