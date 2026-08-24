#!/usr/bin/env node
// Tests for the P0.2 Adapt ontology regression gate (node:test).
// Run: node --test scripts/ci/check-adapt-ontology.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import {
  CANONICAL_SPEC,
  evaluateOntology,
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

test("historical terminology marker suppresses everything after it", () => {
  const text = [
    "clean line",
    "> Historical terminology: this document predates the canonical Adapt ontology. `memory` references below do not define current Adapt product semantics.",
    "Adapt memory system",
    "Taste memory",
  ].join("\n");
  assert.deepEqual(evaluateOntology(text, { path: "docs/hist.md" }), []);
});

test("text before the historical marker still fails", () => {
  const text = [
    "Adapt memories leak into copy",
    "Historical terminology: this document predates the canonical Adapt ontology.",
    "Taste memory",
  ].join("\n");
  const failures = evaluateOntology(text, { path: "docs/hist.md" });
  assert.equal(failures.length, 1);
  assert.equal(failures[0].line, 1);
});

test("path exclusions: canonical spec, research, historical plans", () => {
  assert.equal(isExcludedPath(CANONICAL_SPEC), true);
  assert.equal(isExcludedPath("docs/research/competitors/adapt-analysis.md"), true);
  assert.equal(isExcludedPath("adapt/docs/plans/2026-08-24-adapt-alignment-implementation.md"), true);
  assert.equal(isExcludedPath("README.md"), false);
  assert.equal(isExcludedPath("adapt/README.md"), false);
  // Windows-style separators are normalized.
  assert.equal(isExcludedPath("docs\\research\\x.md"), true);
});

test("real repository scan is clean", () => {
  const targets = scanTargets();
  assert.ok(targets.includes("README.md"));
  assert.ok(targets.includes("adapt/README.md"));
  assert.ok(targets.includes("docs/subsystems/adapt.md"));
  assert.ok(!targets.some((p) => p === CANONICAL_SPEC));
  assert.ok(!targets.some((p) => p.startsWith("docs/research/")));
  const failures = scanRepository();
  assert.deepEqual(failures, []);
});
