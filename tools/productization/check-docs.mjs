#!/usr/bin/env node
// MBR-1001 — Generated product docs and README link gate.
//
// One manually invoked command (no CI in this repository) that fails when:
//   1. any generated product doc (docs/product.md, docs/architecture.md,
//      docs/operations.md, docs/protocol.md) is missing or stale against the
//      source-derived renderers in tools/productization/render-docs.mjs,
//   2. any local markdown link in README.md points at an absent file, or
//   3. the MBR-013 product-truth artifacts are stale (delegated to
//      generate-product-truth.mjs's checkProductTruth, which also covers the
//      README's claimed tool count).
//
// Run:  node tools/productization/check-docs.mjs --check
// Exit: 0 when the tree is consistent, 1 with one FAIL line per finding.

import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkProductTruth, computeProductTruth } from "./generate-product-truth.mjs";
import {
  extractLocalLinks,
  platformStatus,
  renderArchitectureDoc,
  renderOperationsDoc,
  renderProductDoc,
  renderProtocolDoc,
} from "./render-docs.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..", "..");
const README = join(REPO_ROOT, "README.md");
const MATRIX = join(REPO_ROOT, "docs", "membrane", "capability-matrix.v1.json");

/** Generated docs this gate owns, with their renderers. */
export function generatedDocSpecs(truth, platforms) {
  return [
    { path: join(REPO_ROOT, "docs", "product.md"), render: () => renderProductDoc(truth, platforms) },
    { path: join(REPO_ROOT, "docs", "architecture.md"), render: () => renderArchitectureDoc(truth, platforms) },
    { path: join(REPO_ROOT, "docs", "operations.md"), render: () => renderOperationsDoc(truth, platforms) },
    { path: join(REPO_ROOT, "docs", "protocol.md"), render: () => renderProtocolDoc(truth, platforms) },
  ];
}

/**
 * Pure staleness evaluation, exported for testing. Returns one failure string
 * per generated doc whose on-disk content differs from the deterministic render.
 */
export function evaluateGeneratedDocs(specs, onDiskContents) {
  const failures = [];
  for (const spec of specs) {
    const onDisk = onDiskContents.get(spec.path);
    if (onDisk === undefined || onDisk === null) {
      failures.push(`missing generated doc: ${spec.path} (regenerate with generate-product-truth.mjs)`);
    } else if (onDisk !== spec.render()) {
      failures.push(`stale generated doc: ${spec.path} (regenerate with generate-product-truth.mjs)`);
    }
  }
  return failures;
}

/**
 * Pure README link evaluation, exported for testing. `exists` is a predicate
 * over repo-relative link targets. Returns one failure per broken local link.
 */
export function evaluateReadmeLinks(readmeText, exists) {
  return extractLocalLinks(readmeText)
    .filter((target) => !exists(target))
    .map((target) => `broken README link: ${target} (linked from README.md but absent)`);
}

/** Run the full gate against the working tree. Returns { ok, failures }. */
export async function checkDocs() {
  const failures = [];
  const truth = await computeProductTruth();
  const matrix = JSON.parse(await readFile(MATRIX, "utf8"));
  const platforms = platformStatus(matrix);

  // 1. Generated product docs must be present and byte-for-byte current.
  const onDisk = new Map();
  for (const spec of generatedDocSpecs(truth, platforms)) {
    onDisk.set(spec.path, existsSync(spec.path) ? await readFile(spec.path, "utf8") : null);
  }
  failures.push(...evaluateGeneratedDocs(generatedDocSpecs(truth, platforms), onDisk));

  // 2. Every local README link must resolve to an existing file.
  if (!existsSync(README)) {
    failures.push("missing README.md");
  } else {
    const readme = await readFile(README, "utf8");
    failures.push(...evaluateReadmeLinks(readme, (target) => existsSync(join(REPO_ROOT, target))));
  }

  // 3. MBR-013 product truth (tool count, adapters, product-truth.md staleness).
  const truthCheck = await checkProductTruth();
  failures.push(...truthCheck.failures);

  return { ok: failures.length === 0, failures };
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  if (!process.argv.includes("--check")) {
    console.error("usage: node tools/productization/check-docs.mjs --check");
    process.exit(2);
  }
  const result = await checkDocs();
  if (!result.ok) {
    for (const failure of result.failures) console.error(`docs check FAIL: ${failure}`);
    process.exit(1);
  }
  console.log("docs check OK: generated docs current, README links resolve, product truth green");
}
