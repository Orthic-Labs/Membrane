#!/usr/bin/env node
// P0.2 — Adapt ontology terminology regression gate.
//
// Fails when current-product docs describe Adapt, Taste, or Insights as memory,
// per docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md §§14 (P0.2)
// and 15 (terminology firewall). Allowed: Cortex/agent/host memory wording and
// text after the §15.5 historical-terminology marker in explicitly historical
// files only. Plans, design records, and archives are scanned and must carry
// that marker before old terminology. Excluded entirely: research provenance
// and the canonical spec itself (it legitimately enumerates the forbidden
// phrases).

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
/** §15.5 marker: everything after this line in a file is historical evidence. */
const HISTORICAL_MARKER = /Historical terminology:.*predates the canonical Adapt ontology/i;

export const HISTORICAL_PATH_PREFIXES = [
  "adapt/docs/plans/",
  "docs/archive/",
  "docs/design/",
  "docs/plans/",
  "docs/research/",
];

export const EXCLUDED_PATH_PREFIXES = ["docs/research/"];

export const PRIMARY_OVERVIEWS = ["adapt/README.md", "docs/subsystems/adapt.md"];

const REQUIRED_OVERVIEW_IDEAS = [
  {
    label: "Adapt governed behavioral learning",
    pattern: /Adapt(?:\s+is)?\s+(?:Membrane(?:'s|’s)\s+)?governed behavioral[- ]learning subsystem/i,
  },
  { label: "Taste user-backed preferences", pattern: /Taste[\s\S]{0,120}user-backed preferences/i },
  {
    label: "Insights evidence-backed failures/gotchas",
    pattern: /Insights[\s\S]{0,160}evidence-backed[\s\S]{0,100}(?:failures?|gotchas?)/i,
  },
  {
    label: "Cortex durable ownership",
    pattern: /Cortex[\s\S]{0,160}durable admission[\s\S]{0,100}lifecycle[\s\S]{0,100}storage[\s\S]{0,100}retrieval[\s\S]{0,100}delivery/i,
  },
];

/**
 * Path exclusions: research provenance and the canonical spec. Historical
 * product documents remain in the scan. `relPath` uses POSIX separators.
 */
export function isExcludedPath(relPath) {
  const p = relPath.replaceAll("\\", "/");
  return p === CANONICAL_SPEC || EXCLUDED_PATH_PREFIXES.some((prefix) => p.startsWith(prefix));
}

export function isHistoricalPath(relPath) {
  const p = relPath.replaceAll("\\", "/");
  return p === CANONICAL_SPEC || HISTORICAL_PATH_PREFIXES.some((prefix) => p.startsWith(prefix));
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
      if (!isHistoricalPath(path)) {
        failures.push({ path, line: i + 1, phrase: "historical marker outside historical path" });
        continue;
      }
      historical = true;
      continue;
    }
    if (historical) continue;
    for (const { pattern, label } of FORBIDDEN_PATTERNS) {
      const match = pattern.exec(line);
      if (!match) continue;
      const genericMemoryPhrase = label === "admitted memory/memories" || label === "learned memory/memories";
      // Generic phrases are legitimate when their grammatical lead-in names
      // Cortex/agent/host memory. Text later on the same line must not hide an
      // earlier forbidden Adapt claim.
      const leadIn = line.slice(Math.max(0, match.index - 96), match.index);
      if (genericMemoryPhrase && /\b(?:Cortex|agent|host)\b/i.test(leadIn)) continue;
      failures.push({ path, line: i + 1, phrase: label });
    }
  }
  return failures;
}

export function evaluatePrimaryOverview(text, { path }) {
  if (!PRIMARY_OVERVIEWS.includes(path.replaceAll("\\", "/"))) return [];
  return REQUIRED_OVERVIEW_IDEAS.filter(({ pattern }) => !pattern.test(text)).map(({ label }) => ({
    path,
    line: 1,
    phrase: `missing canonical idea: ${label}`,
  }));
}

function mdFilesUnder(root, dirRel) {
  const abs = join(root, dirRel);
  const out = [];
  if (!existsSync(abs)) return out;
  for (const entry of readdirSync(abs, { withFileTypes: true })) {
    const child = `${dirRel}/${entry.name}`;
    if (entry.isDirectory()) out.push(...mdFilesUnder(root, child));
    else if (entry.isFile() && entry.name.endsWith(".md")) out.push(child);
  }
  return out;
}

/** Product Markdown inventory. Research provenance is excluded explicitly. */
export function scanTargets(root = REPO_ROOT) {
  return [...new Set([
    "README.md",
    ...mdFilesUnder(root, "docs"),
    "adapt/README.md",
    ...mdFilesUnder(root, "adapt/docs"),
    "migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md",
  ].filter((p) => !isExcludedPath(p)))].sort();
}

/** Scan all current-product targets on disk. Returns the combined failure list. */
export function scanRepository(root = REPO_ROOT) {
  const failures = [];
  for (const rel of scanTargets(root)) {
    const abs = join(root, rel);
    if (!existsSync(abs)) continue;
    const text = readFileSync(abs, "utf8");
    failures.push(...evaluateOntology(text, { path: rel }));
    failures.push(...evaluatePrimaryOverview(text, { path: rel }));
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
