#!/usr/bin/env node
// P0.2 — Adapt ontology terminology regression gate.
//
// Fails when current-product docs describe Adapt, Taste, or Insights as memory,
// per docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md §§14 (P0.2)
// and 15 (terminology firewall). Allowed: Cortex/agent/host memory wording and
// text after the §15.5 historical-terminology marker. Excluded entirely:
// research files, historical plans, and the canonical spec itself (it
// legitimately enumerates the forbidden phrases).

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const REPO_ROOT = fileURLToPath(new URL("../..", import.meta.url));

/** The canonical spec enumerates forbidden phrases; scanning it would self-report. */
export const CANONICAL_SPEC = "docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md";

const FORBIDDEN_PATTERNS = [
  { pattern: /\bcontinuous coding-taste memory\b/i, label: "continuous coding-taste memory" },
  // Lookbehind keeps the longer variant from double-reporting via this pattern.
  { pattern: /\b(?<!continuous )coding-taste memory\b/i, label: "coding-taste memory" },
  { pattern: /\bAdapt memory (?:system|substrate|control plane)\b/i, label: "Adapt memory system/substrate/control plane" },
  // Lookbehind keeps "coding-taste memory" from double-reporting via this pattern.
  { pattern: /\b(?<!coding-)Taste memory\b/i, label: "Taste memory" },
  { pattern: /\bInsights? memory\b/i, label: "Insights/Insight memory" },
  { pattern: /\bAdapt memories\b/i, label: "Adapt memories" },
  { pattern: /\badmitted memor(?:y|ies)\b/i, label: "admitted memory/memories" },
  { pattern: /\blearned memor(?:y|ies)\b/i, label: "learned memory/memories" },
];

/** Line-level allowlist (§15.3): legitimate non-Adapt uses of "memory". */
const ALLOWED_SUBSTRINGS = [
  "cortex durable memory",
  "durable-memory",
  "agent memory",
  "host memory",
  "memory file",
  "memory bank",
  "memory_sentinel",
  "memory_id",
  "competitor memory",
];

/** §15.5 marker: everything after this line in a file is historical evidence. */
const HISTORICAL_MARKER = /Historical terminology:.*predates the canonical Adapt ontology/i;

/**
 * Path exclusions: research provenance, marked-historical plans, and the
 * canonical spec. `relPath` uses POSIX separators.
 */
export function isExcludedPath(relPath) {
  const p = relPath.replaceAll("\\", "/");
  return (
    p === CANONICAL_SPEC ||
    p.startsWith("docs/research/") ||
    p.startsWith("adapt/docs/plans/")
  );
}

/**
 * Pure ontology evaluation for one document. Returns failures as
 * [{ path, line, phrase }] with 1-based line numbers.
 */
export function evaluateOntology(text, { path }) {
  const failures = [];
  let historical = false;
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (HISTORICAL_MARKER.test(line)) {
      historical = true;
      continue;
    }
    if (historical) continue;
    for (const { pattern, label } of FORBIDDEN_PATTERNS) {
      const match = pattern.exec(line);
      if (!match) continue;
      const genericMemoryPhrase = label === "admitted memory/memories" || label === "learned memory/memories";
      const lowered = line.toLowerCase();
      if (genericMemoryPhrase && ALLOWED_SUBSTRINGS.some((allowed) => lowered.includes(allowed))) continue;
      failures.push({ path, line: i + 1, phrase: label });
    }
  }
  return failures;
}

function mdFilesIn(dirRel) {
  const abs = join(REPO_ROOT, dirRel);
  if (!existsSync(abs)) return [];
  return readdirSync(abs, { withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith(".md"))
    .map((e) => `${dirRel}/${e.name}`);
}

function mdFilesUnder(dirRel) {
  const abs = join(REPO_ROOT, dirRel);
  const out = [];
  if (!existsSync(abs)) return out;
  for (const entry of readdirSync(abs, { withFileTypes: true })) {
    const child = `${dirRel}/${entry.name}`;
    if (entry.isDirectory()) out.push(...mdFilesUnder(child));
    else if (entry.isFile() && entry.name.endsWith(".md")) out.push(child);
  }
  return out;
}

/** Current-product doc targets (packet §3.A). Read-only inventory; missing paths are skipped. */
export function scanTargets() {
  return [
    "README.md",
    ...mdFilesIn("docs"),
    ...mdFilesIn("docs/subsystems"),
    ...mdFilesUnder("docs/hub"),
    ...mdFilesIn("docs/membrane"),
    "adapt/README.md",
    ...mdFilesIn("adapt/docs"), // top level only; adapt/docs/plans is excluded by isExcludedPath anyway
    "migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md",
  ].filter((p) => !isExcludedPath(p));
}

/** Scan all current-product targets on disk. Returns the combined failure list. */
export function scanRepository(root = REPO_ROOT) {
  const failures = [];
  for (const rel of scanTargets()) {
    const abs = join(root, rel);
    if (!existsSync(abs)) continue;
    failures.push(...evaluateOntology(readFileSync(abs, "utf8"), { path: rel }));
  }
  return failures;
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const failures = scanRepository();
  if (failures.length > 0) {
    console.error(`adapt-ontology check FAIL (${failures.length}):`);
    for (const f of failures) console.error(`  ${f.path}:${f.line}: ${f.phrase}`);
    process.exit(1);
  }
  console.log(`adapt-ontology check OK: ${scanTargets().length} current-product docs clean`);
}
