#!/usr/bin/env node
// Tests for the P0.2 Adapt ontology regression gate (node:test).
// Run: node --test scripts/ci/check-adapt-ontology.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  CANONICAL_SPEC,
  evaluateOntology,
  evaluatePrimaryOverview,
  isHistoricalPath,
  isExcludedPath,
  scanRepository,
  scanTargets,
} from "./check-adapt-ontology.mjs";

test("forbidden phrases fire with path and line", () => {
  const phrases = [
    "Adapt memory system",
    "Adapt memory substrate",
    "Adapt memory control plane",
    "Taste memory",
    "coding-taste memory",
    "continuous coding-taste memory",
    "Insights memory",
    "Insight memory",
    "Adapt memories",
    "admitted memory",
    "admitted memories",
    "learned memories",
  ];
  for (const phrase of phrases) {
    const failures = evaluateOntology(`intro line\nuses ${phrase} here\n`, { path: "docs/x.md" });
    assert.equal(failures.length, 1, `expected one failure for: ${phrase}`);
    assert.equal(failures[0].path, "docs/x.md");
    assert.equal(failures[0].line, 2);
  }
});

test("allowed Cortex / agent / host memory wording passes", () => {
  const text = [
    "Cortex is the durable-memory substrate.",
    "Cortex owns durable memory admission and retrieval.",
    "Agent memory provides contextual recall.",
    "Host memory features are external.",
    "Memory files are one source of prompt overhead.",
    "Cline memory banks are competitor systems.",
    "Producers include memory_sentinel_producer.rs.",
    "Records carry memory_id fields.",
  ].join("\n");
  assert.deepEqual(evaluateOntology(text, { path: "docs/x.md" }), []);
});

test("allowed wording on a line does not hide a forbidden Adapt phrase", () => {
  const failures = evaluateOntology(
    "Cortex durable memory is separate from the Taste memory.",
    { path: "docs/x.md" },
  );
  assert.equal(failures.length, 1);
  assert.equal(failures[0].phrase, "Taste memory");
});

test("allowed Cortex wording after a forbidden generic phrase does not hide it", () => {
  const failures = evaluateOntology(
    "Adapt owns admitted memory; Cortex durable memory is separate.",
    { path: "docs/x.md" },
  );
  assert.equal(failures.length, 1);
  assert.equal(failures[0].phrase, "admitted memory/memories");
  assert.deepEqual(
    evaluateOntology("In Cortex, records become admitted memory.", { path: "docs/x.md" }),
    [],
  );
});

test("historical terminology marker suppresses only explicitly historical paths", () => {
  const text = [
    "clean line",
    "> Historical terminology: this document predates the canonical Adapt ontology. `memory` references below do not define current Adapt product semantics.",
    "Adapt memory system",
    "Taste memory",
  ].join("\n");
  assert.deepEqual(evaluateOntology(text, { path: "docs/plans/hist.md" }), []);
  const current = evaluateOntology(text, { path: "docs/subsystems/current.md" });
  assert.equal(current[0].phrase, "historical marker outside historical path");
  assert.ok(current.some((failure) => failure.phrase === "Adapt memory system/substrate/control plane"));
});

test("text before the historical marker still fails", () => {
  const text = [
    "Adapt memories leak into copy",
    "Historical terminology: this document predates the canonical Adapt ontology.",
    "Taste memory",
  ].join("\n");
  const failures = evaluateOntology(text, { path: "docs/plans/hist.md" });
  assert.equal(failures.length, 1);
  assert.equal(failures[0].line, 1);
});

test("path exclusions: canonical spec, research, historical plans", () => {
  assert.equal(isExcludedPath(CANONICAL_SPEC), true);
  assert.equal(isExcludedPath("docs/research/legacy-source-corpus/competitors/adapt-analysis.md"), true);
  assert.equal(isExcludedPath("adapt/docs/plans/2026-08-24-adapt-alignment-implementation.md"), false);
  assert.equal(isExcludedPath("docs/plans/old.md"), false);
  assert.equal(isExcludedPath("docs/design/old.md"), false);
  assert.equal(isExcludedPath("docs/archive/old.md"), false);
  assert.equal(isExcludedPath("README.md"), false);
  assert.equal(isExcludedPath("adapt/README.md"), false);
  // Windows-style separators are normalized.
  assert.equal(isExcludedPath("docs\\research\\legacy-source-corpus\\x.md"), true);
  assert.equal(isHistoricalPath("docs/plans/x.md"), true);
  assert.equal(isHistoricalPath("docs/architecture/subsystems/adapt.md"), true);
});

test("scan targets inventories the supplied root", () => {
  const root = mkdtempSync(join(tmpdir(), "adapt-ontology-"));
  try {
    mkdirSync(join(root, "docs", "nested"), { recursive: true });
    writeFileSync(join(root, "docs", "nested", "only-in-fixture.md"), "clean\n");
    assert.ok(scanTargets(root).includes("docs/nested/only-in-fixture.md"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("primary overviews must state all four canonical ideas", () => {
  const complete = [
    "Adapt is Membrane's governed behavioral-learning subsystem.",
    "Taste learns user-backed preferences.",
    "Insights learns evidence-backed agent failures and gotchas.",
    "Cortex owns durable admission, lifecycle, storage, retrieval, and delivery.",
  ].join("\n");
  assert.deepEqual(evaluatePrimaryOverview(complete, { path: "adapt/README.md" }), []);
  const missing = evaluatePrimaryOverview("Adapt is a tool.", { path: "adapt/README.md" });
  assert.equal(missing.length, 4);
  assert.deepEqual(evaluatePrimaryOverview("irrelevant", { path: "docs/reference/cli/README.md" }), []);
});

test("real repository scan is clean", () => {
  const targets = scanTargets();
  assert.ok(targets.includes("README.md"));
  assert.ok(targets.includes("adapt/README.md"));
  assert.ok(!targets.includes("docs/architecture/subsystems/adapt.md"));
  assert.ok(!targets.some((p) => p === CANONICAL_SPEC));
  assert.ok(!targets.some((p) => p.startsWith("docs/research/legacy-source-corpus/")));
  assert.ok(!targets.some((p) => p.startsWith("docs/plans/")));
  assert.deepEqual(targets, [...targets].sort());
  assert.equal(new Set(targets).size, targets.length);
  for (const path of [
    "docs/reference/cli/README.md",
    "docs/architecture/runtime/tray-daemon-contract.md",
    "docs/product/installation/contract.md",
    "docs/product/troubleshooting/hub-alerts.md",
  ]) {
    assert.ok(targets.includes(path), `missing current-product target ${path}`);
  }
  const failures = scanRepository();
  assert.deepEqual(failures, []);
});
