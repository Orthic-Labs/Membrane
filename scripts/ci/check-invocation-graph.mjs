#!/usr/bin/env node
// Invocation-graph + legacy-ledger reconciliation checker (migration spec N0,
// sections 16.2/19/19.1). Validates that:
//   1. migration/native-rust/invocation-graph.json is fresh against the current
//      tracked tree and its reachability derivation reproduces;
//   2. runtime-language-manifest.json rows agree with graph-derived
//      production reachability (the manifest is a projection of the graph);
//   3. executable-ledger.json stays reconciled + superseded and no gate
//      consumes it;
//   4. no native-only-seal.json exists unless every gate honestly passes.
//
// CLI modes:
//   node scripts/ci/check-invocation-graph.mjs          -> human report
//   node scripts/ci/check-invocation-graph.mjs --json   -> machine-readable report

import { readFileSync } from "node:fs";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  GRAPH_REL,
  MANIFEST_REL,
  RECONCILIATION_REL,
  TRAVERSABLE_BOUNDARIES,
  buildGraph,
  isExecutableCandidate,
  loadTrackedFiles,
  productionEntrypoints,
} from "./build-invocation-graph.mjs";

export const SEAL_REL = "migration/native-rust/native-only-seal.json";

export function deriveReachability(graph) {
  const adjacency = new Map();
  for (const e of graph.edges ?? []) {
    if (!TRAVERSABLE_BOUNDARIES.has(e.boundary)) continue;
    if (!adjacency.has(e.from)) adjacency.set(e.from, []);
    adjacency.get(e.from).push(e);
  }
  const reach = new Map();
  const queue = [];
  for (const s of graph.productionEntrypoints ?? []) {
    reach.set(s.id, { seed: s.id, hops: 0 });
    queue.push(s.id);
  }
  while (queue.length) {
    const cur = queue.shift();
    const { seed, hops } = reach.get(cur);
    for (const e of adjacency.get(cur) ?? []) {
      if (!reach.has(e.to)) {
        reach.set(e.to, { seed, hops: hops + 1 });
        queue.push(e.to);
      }
    }
  }
  return reach;
}

export function rowAgreement(manifestRow, reachableSet) {
  // External typed services are validated by boundary presence, not by
  // Membrane-side file execution (spec section 1.3): their files belong to the
  // external product's own inventory.
  if (manifestRow.runtime === "external") return { agrees: true, external: true };
  const expected = (manifestRow.files ?? []).some((f) => reachableSet.has(f));
  return { agrees: expected === !!manifestRow.production_reachable, expected };
}

export function validateInvocationGraph({ root, graph, manifest, reconciliation, trackedFiles }) {
  const errors = [];
  const warnings = [];
  const add = (code, message, path) => errors.push({ code, message, ...(path ? { path } : {}) });

  if (!graph || graph.artifact !== "membrane.invocation-graph" || graph.schemaVersion !== 2) {
    add("GRAPH_SCHEMA_MISMATCH", "invocation-graph artifact/schemaVersion mismatch — regenerate with build-invocation-graph.mjs --write");
    return { errors, warnings };
  }

  // 1. Node freshness against current tracked executables.
  const nodeIds = new Set((graph.nodes ?? []).map((n) => n.id));
  const discovered = trackedFiles.filter((f) => isExecutableCandidate(f, root));
  for (const f of discovered) {
    if (!nodeIds.has(f)) add("STALE_GRAPH_MISSING_NODE", "tracked executable missing from invocation graph", f);
  }
  const trackedSet = new Set(trackedFiles);
  for (const n of graph.nodes ?? []) {
    if (n.kind === "tracked-executable" && !trackedSet.has(n.id)) {
      add("STALE_GRAPH_GHOST_NODE", "graph lists a tracked executable that git no longer tracks", n.id);
    }
  }

  // 2. Edge integrity + seed integrity.
  for (const e of graph.edges ?? []) {
    if (!nodeIds.has(e.from)) add("EDGE_UNKNOWN_ENDPOINT", `edge endpoint not a node`, e.id ?? e.from);
    if (!nodeIds.has(e.to)) add("EDGE_UNKNOWN_ENDPOINT", `edge endpoint not a node`, e.id ?? e.to);
    if (!TRAVERSABLE_BOUNDARIES.has(e.boundary) && e.boundary !== "path-reference" && e.boundary !== "data") {
      add("EDGE_UNKNOWN_BOUNDARY", `boundary '${e.boundary}' neither traversable nor recorded weak evidence`, e.id ?? e.from);
    }
    if (!e.origin || !["scanned", "curated"].includes(e.origin)) {
      add("EDGE_MISSING_ORIGIN", "edge lacks scanned/curated origin", e.id ?? e.from);
    }
    if (!e.evidence || !e.evidence.length) {
      add("EDGE_MISSING_EVIDENCE", "edge lacks evidence citation", e.id ?? e.from);
    }
  }
  const declaredSeeds = new Set((graph.productionEntrypoints ?? []).map((s) => s.id));
  for (const s of declaredSeeds) {
    if (!trackedSet.has(s)) {
      add("SEED_INVALID", `declared production entrypoint seed is missing or untracked: ${s}`);
    }
  }
  // Canonical-seed coverage is checked against what git actually tracks here,
  // so partial synthetic trees (tests) are not punished.
  const expectedSeeds = productionEntrypoints().map((s) => s.id);
  for (const s of expectedSeeds) {
    if (!declaredSeeds.has(s) && trackedSet.has(s)) {
      add("SEED_MISSING_FROM_GRAPH", `tracked canonical entrypoint not declared as a seed: ${s}`);
    }
  }

  // 3. Reachability reproduction.
  const reach = deriveReachability(graph);
  for (const n of graph.nodes ?? []) {
    if (!!reach.has(n.id) !== !!n.production_reachable) {
      add(
        "REACHABILITY_NOT_REPRODUCIBLE",
        `node production_reachable=${n.production_reachable} but derived=${reach.has(n.id)}`,
        n.id,
      );
    }
  }
  const reachableSet = new Set(
    (graph.nodes ?? [])
      .filter((n) => n.production_reachable)
      .map((n) => n.id),
  );

  // 4. Manifest projection agreement.
  for (const row of manifest?.rows ?? []) {
    const verdict = rowAgreement(row, reachableSet);
    if (!verdict.agrees) {
      add(
        "MANIFEST_ROW_DISAGREES_WITH_GRAPH",
        `row production_reachable=${row.production_reachable} but graph-derived=${verdict.expected} (regenerate manifest after aligning runtime-policy rules)`,
        row.id,
      );
    }
  }
  // External boundary sanity: blueprint consumed only via typed daemon boundary.
  const externalNodes = new Set((graph.nodes ?? []).filter((n) => n.kind.startsWith("external")).map((n) => n.id));
  for (const row of manifest?.rows ?? []) {
    if (row.runtime === "external" && row.production_reachable) {
      const covered = (row.files ?? []).length > 0 || externalNodes.size > 0;
      if (!covered) add("EXTERNAL_BOUNDARY_MISSING", "external row has neither files nor an external boundary node", row.id);
    }
  }

  // 5. Legacy ledger reconciliation.
  if (!reconciliation || reconciliation.status !== "superseded") {
    add("LEGACY_LEDGER_NOT_SUPERSEDED", "legacy ledger reconciliation missing or not marked superseded");
  } else {
    const legacyCount = Number(reconciliation.legacyArtifactCount ?? 0);
    const seen = new Set();
    for (const m of reconciliation.mappings ?? []) {
      if (seen.has(m.legacyId)) add("RECONCILIATION_DUPLICATE_MAPPING", "artifact mapped twice", m.legacyId);
      seen.add(m.legacyId);
    }
    if (legacyCount && seen.size !== legacyCount) {
      add(
        "RECONCILIATION_INCOMPLETE",
        `reconciliation covers ${seen.size}/${legacyCount} legacy artifacts`,
      );
    }
    if ((reconciliation.gatesConsumingLegacyLedger ?? []).length > 0) {
      add(
        "GATE_CONSUMES_LEGACY_LEDGER",
        `a gate still consumes executable-ledger.json: ${reconciliation.gatesConsumingLegacyLedger.join(", ")}`,
      );
    }
  }

  // 6. Honest classification: no premature native-only seal.
  if (existsSync(join(root, SEAL_REL))) {
    let prodInterpreters = 0;
    for (const row of manifest?.rows ?? []) {
      if (row.production_reachable && ["python", "node"].includes(row.runtime)) prodInterpreters++;
    }
    const unresolved = (graph.unresolvedReferences ?? []).length;
    if (prodInterpreters > 0 || unresolved > 0) {
      add(
        "NATIVE_ONLY_SEAL_PREMATURE",
        `native-only-seal.json exists while ${prodInterpreters} production interpreter row(s) and ${unresolved} unresolved reference(s) remain`,
        SEAL_REL,
      );
    }
  }

  return { errors, warnings, reachableSet };
}

function main(argv) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
  const jsonOut = argv.includes("--json");

  const load = (rel) => JSON.parse(readFileSync(join(root, rel), "utf8"));
  let graph, manifest, reconciliation;
  try {
    graph = load(GRAPH_REL);
  } catch {
    process.stderr.write(`invocation graph missing/unreadable: ${GRAPH_REL}\n`);
    return 2;
  }
  try {
    manifest = load(MANIFEST_REL);
  } catch {
    manifest = null;
  }
  try {
    reconciliation = load(RECONCILIATION_REL);
  } catch {
    reconciliation = null;
  }

  const trackedFiles = loadTrackedFiles(root);
  const { errors, warnings } = validateInvocationGraph({ root, graph, manifest, reconciliation, trackedFiles });

  if (jsonOut) {
    process.stdout.write(`${JSON.stringify({ ok: errors.length === 0, errors, warnings }, null, 2)}\n`);
  } else {
    const t = manifest?.totals ?? {};
    process.stdout.write(
      `invocation-graph check [baseline ${graph.baselineCommit ?? "?"}]\n` +
      `  nodes=${(graph.nodes ?? []).length} edges=${(graph.edges ?? []).length} ` +
      `reachableFiles=${(graph.derivedProductionFiles ?? []).length} unresolvedRefs=${(graph.unresolvedReferences ?? []).length}\n` +
      `  manifestRows=${t.rows ?? "?"} errors=${errors.length} warnings=${warnings.length}\n`,
    );
    for (const e of errors) process.stdout.write(`  ERROR ${e.code}: ${e.message}${e.path ? ` (${e.path})` : ""}\n`);
    for (const w of warnings) process.stdout.write(`  WARN  ${w.code}: ${w.message}\n`);
    process.stdout.write(`  native-only seal: ${existsSync(join(root, SEAL_REL)) ? "PRESENT" : "not issued"} (honest status)\n`);
  }
  return errors.length > 0 ? 1 : 0;
}

if (process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href) {
  process.exitCode = main(process.argv.slice(2));
}
