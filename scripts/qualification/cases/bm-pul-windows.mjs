#!/usr/bin/env node
// scripts/qualification/cases/bm-pul-windows.mjs — federation-catalog lane case module.
//
// r5.1 amendment (blueprint-membrane-acceptance.json): BM01, BM02, BM08, BM10
// moved out of scripts/qualification/cases/pul-windows.mjs because that
// module's federation-catalog touch budget was exhausted at checkpoint I1.
// pul-windows.mjs stays closed and is not edited by this module.
//
// Contract: each export is called by scripts/qualification/run.mjs
// runOneRegistryCase as caseFunction({ row, workspaceRoot, profile, platform,
// evidencePath }) and must return { status: "passed"|<anything else>,
// evidenceKind, detail, reason } where evidenceKind is one of run.mjs
// EVIDENCE_KINDS (see the sibling bpt-windows.mjs / ctx-windows.mjs
// convention). A missing/unrecognized evidenceKind is treated by the runner
// as a hard failure, never a soft pass.
//
// Honesty policy: every BM row here is registered in
// blueprint-membrane-acceptance.json with implementationStatus
// "IMPLEMENT_THEN_RUN; no runtime result claimed" and executionOwner
// "Membrane integration owner only" — the real native-boundary,
// degradation-propagation, evidence-admission and evidence-journey behavior
// these rows describe spans membrane-federation/membrane-runtime/
// membrane-blueprint wiring this lane does not own or edit. This module
// therefore never fabricates a "passed" functional result. Each export
// returns a typed "insufficient" outcome citing the concrete acceptance
// text and negative controls the integration owner's installed-path run
// must satisfy, plus a structural check (source-evidenceKind) of whether
// the canonical collaborator crates named in the row are present at all —
// consistent with the sibling pul-windows.mjs structural-attestation
// convention (never a functional proof).

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

function resolveRoot(context) {
  return (context && context.workspaceRoot) || (context && context.root) || REPO_ROOT;
}

function dirExists(root, relPath) {
  const abs = join(root, relPath);
  try {
    return existsSync(abs) && readFileSync(abs, "utf8").length >= 0;
  } catch {
    return existsSync(abs);
  }
}

// Structural collaborator-presence check: the row's acceptance text names
// concrete crates/paths this behavior must be wired through. Presence is
// necessary but never sufficient for a "passed" result — only the
// integration owner's real installed-path run (per executionOwner) can
// close BM01/BM02/BM08/BM10.
function bmRow(id, title, acceptance, negativeControls, collaboratorPaths, context) {
  const root = resolveRoot(context);
  const evidence = collaboratorPaths.map((p) => ({ path: p, present: dirExists(root, p) }));
  const missing = evidence.filter((e) => !e.present).map((e) => e.path);
  return {
    status: "insufficient",
    evidenceKind: "source",
    detail: { id, title, acceptance, negativeControls, evidence, missing },
    reason:
      id +
      ": " +
      title +
      " requires a real installed-path run (executionOwner: Membrane integration owner only; implementationStatus: IMPLEMENT_THEN_RUN, no runtime result claimed here). " +
      (missing.length === 0
        ? "Named collaborator artifact(s) present (structural only, not a functional proof)."
        : "Missing named collaborator artifact(s): " + missing.join(", ") + "."),
  };
}

export function BM01(context) {
  return bmRow(
    "BM01",
    "Real native boundary",
    "Fixture repository -> real native Blueprint operation -> real BlueprintClient -> real runtime federation/Pull -> final packet and receipt. Recall must produce source-bound candidates under one Blueprint-owned contract; Resolve must agree on target fields. Exercise native producer, not fabricated NativeApi response. Preserve supported public schemas, IDs and sourceRef; no adapter-side semantic graph or ranking. Include ambiguous/unknown target and cancellation.",
    [
      "Fabricated NativeApi response passing as producer fails.",
      "Resolve field disagreement between query.rs and blueprint_client.rs fails.",
      "Ambiguous/unknown target and cancellation must return typed outcomes.",
    ],
    [
      "engine/crates/membrane-blueprint/src",
      "engine/crates/membrane-client/src",
      "engine/crates/membrane-federation/src",
    ],
    context,
  );
}

export function BM02(context) {
  return bmRow(
    "BM02",
    "Degradation propagation",
    "Native resolution, coverage, freshness and omissions must govern outer SourceResponse completeness/warnings, final admission and rendered packet. Test partial, stale, unsupported, timed out and unresolved dynamic results; nested raw JSON alone fails. Provider diagnostics include status, generation, freshness, latency, cancellation, errors, fallback and candidate count.",
    [
      "Nested raw JSON with outer complete=true fails.",
      "Partial/stale/timed-out/unresolved dynamic results must degrade outer completeness and warnings.",
    ],
    ["engine/crates/membrane-federation/src", "engine/crates/membrane-runtime/src"],
    context,
  );
}

export function BM08(context) {
  return bmRow(
    "BM08",
    "Evidence admission integrity",
    "Keep query-driven retrieval distinct from tiny governed baseline projection, each with explicit applicability, scope, authority, budget and omission reasons. Task packs are splittable between complete evidence units; qualifiers, required contradiction pairs, provenance and complete Blueprint evidence paths remain atomic. Budget reduction must not retain a misleading claim fragment. Ambiguity drives explicit allow/continue/block/noop, never silent top-1 collapse.",
    [
      "Silent top-1 collapse under ambiguity fails (Z14).",
      "Misleading claim fragment retained after budget cut fails (M11).",
      "Bundle selection that ignores independent admission comparison fails (Z13).",
    ],
    [
      "engine/crates/cortex/src",
      "engine/crates/membrane-blueprint/src",
      "engine/crates/membrane-runtime/src/ledger",
    ],
    context,
  );
}

export function BM10(context) {
  return bmRow(
    "BM10",
    "Early evidence journey",
    "Extend existing CandidateJourneyV1, not separate diagnostics. At first coherent real-path checkpoint collect stable evidence IDs, conversion/omission receipts and independently specified expected evidence. Map NOT_DISCOVERED, DISCOVERED_REJECTED, DISCOVERED_BUDGET_DROPPED, STALE, ADAPTER_DROPPED, EXECUTION_FAILURE. Emitted != host included != model used != helped. DELIVERED_IGNORED requires supported observation; absent citation/read is not proof and unknown remains unknown.",
    [
      "CandidateJourneyV1 unable to express NOT_DISCOVERED, DISCOVERED_REJECTED, DISCOVERED_BUDGET_DROPPED, STALE, ADAPTER_DROPPED, EXECUTION_FAILURE fails (M08, Z21).",
      "DELIVERED_IGNORED without supported observation fails.",
      "Baseline instrumentation absent at integration checkpoint I1 fails (M07).",
    ],
    ["engine/crates/membrane-runtime/src", "engine/crates/membrane-federation/src"],
    context,
  );
}

export const BM_CASES = { BM01, BM02, BM08, BM10 };
export default BM_CASES;
