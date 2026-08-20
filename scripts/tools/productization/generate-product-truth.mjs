#!/usr/bin/env node
// MBR-013 — Generate tool, route, adapter, and documentation truth.
//
// The product's public surface (MCP tools, adapters, and the generated docs that
// describe them) must be DERIVED from source, never hand-written and left to
// drift. This generator reads the live tool inventory (mcp/server.mjs) and the
// vendored adapter capability matrix (docs/membrane/capability-matrix.v1.json),
// and emits canonical truth artifacts under schemas/operations/ and docs/.
//
// `--check` is the manually invoked book-gate product-truth check (the no-CI
// override replaces "CI fails" with this local command). It exits non-zero when
// the generated artifacts are stale, when the README's claimed tool count
// disagrees with the source (e.g. README says "six tools" while the source
// exposes nine), or when a generated doc the README links to is absent.

import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { TOOLS } from "../../../mcp/server.mjs";
import {
  platformStatus,
  renderArchitectureDoc,
  renderOperationsDoc,
  renderProductDoc,
  renderProtocolDoc,
} from "./render-docs.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..", "..", "..");
const TRUTH_JSON = join(REPO_ROOT, "schemas", "registry", "product-truth.json");
const TRUTH_DOC = join(REPO_ROOT, "docs", "product-truth.md");
const README = join(REPO_ROOT, "README.md");
const MATRIX = join(REPO_ROOT, "docs", "membrane", "capability-matrix.v1.json");
const MANIFEST = join(REPO_ROOT, "docs", "design", "MEMBRANE-CURRENT-STATE-MANIFEST.json");

const TRUTH_SCHEMA = "membrane.product-truth.v1";
const AXIS_IDS = ["pull", "push", "cortex", "blueprint", "guide", "adapt"];

// Number words the README prose may use for the tool count claim.
const NUMBER_WORDS = { six: 6, seven: 7, eight: 8, nine: 9, ten: 10, eleven: 11, twelve: 12 };

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((k) => `${JSON.stringify(k)}:${canonicalJson(value[k])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

/** Validate the canonical six-axis and runtime-boundary declarations. */
function capabilityDeclarations(matrix) {
  const axes = matrix.axes;
  if (!Array.isArray(axes) || axes.length !== AXIS_IDS.length) {
    throw new Error(`capability matrix must declare exactly six axes: ${AXIS_IDS.join(", ")}`);
  }
  const ids = axes.map((axis) => axis?.id);
  if (ids.some((id, index) => id !== AXIS_IDS[index])) {
    throw new Error(`capability matrix axes must be ordered ${AXIS_IDS.join(", ")}`);
  }
  for (const axis of axes) {
    if (!axis || typeof axis.label !== "string" || typeof axis.description !== "string") {
      throw new Error(`capability matrix axis ${axis?.id ?? "unknown"} is incomplete`);
    }
  }
  if (matrix.current_target !== "macOS") {
    throw new Error("capability matrix current_target must be macOS");
  }
  if (matrix.cortex_scope !== "durable-memory-only") {
    throw new Error("capability matrix cortex_scope must be durable-memory-only");
  }
  if (matrix.resident_service_authority !== "hub") {
    throw new Error("capability matrix resident_service_authority must be hub");
  }
  return {
    axes: AXIS_IDS,
    axisDefinitions: axes,
    currentTarget: matrix.current_target,
    cortexScope: matrix.cortex_scope,
    residentServiceAuthority: matrix.resident_service_authority,
  };
}

/** Compute the product truth from live source. Deterministic — no timestamps. */
export async function computeProductTruth() {
  const tools = TOOLS.map((tool) => tool.name).sort();
  const matrix = JSON.parse(await readFile(MATRIX, "utf8"));
  const adapters = Object.keys(matrix.hosts || {}).sort();
  const declarations = capabilityDeclarations(matrix);
  return {
    schema: TRUTH_SCHEMA,
    ...declarations,
    toolCount: tools.length,
    tools,
    adapterCount: adapters.length,
    adapters,
    generatedFrom: ["mcp/server.mjs", "docs/membrane/capability-matrix.v1.json"],
  };
}

/** Read the platform status (support tiers) from the capability matrix. */
export async function computePlatformStatus() {
  const matrix = JSON.parse(await readFile(MATRIX, "utf8"));
  return platformStatus(matrix);
}

function renderTruthJson(truth) {
  // Pretty-printed but key-sorted canonical JSON, stable across runs.
  const sorted = JSON.parse(canonicalJson(truth));
  return `${JSON.stringify(sorted, null, 2)}\n`;
}

// MBR-1001 — The generated product docs and the manifest's productTruth block
// are derived from the same truth/platform computation, so regeneration is
// deterministic and --check can prove them current.

/** Map of generated doc path -> rendered content. */
export function renderGeneratedDocs(truth, platforms) {
  return new Map([
    [join(REPO_ROOT, "docs", "product.md"), renderProductDoc(truth, platforms)],
    [join(REPO_ROOT, "docs", "architecture.md"), renderArchitectureDoc(truth, platforms)],
    [join(REPO_ROOT, "docs", "operations.md"), renderOperationsDoc(truth, platforms)],
    [join(REPO_ROOT, "docs", "protocol.md"), renderProtocolDoc(truth, platforms)],
  ]);
}

/**
 * Render the manifest with its productTruth block derived from source. All
 * other manifest content is preserved byte-for-byte (key order retained);
 * only the derived block is replaced.
 */
export function renderManifest(manifestText, truth, platforms) {
  const manifest = JSON.parse(manifestText);
  manifest.productTruth = {
    axes: truth.axes,
    axisDefinitions: truth.axisDefinitions,
    currentTarget: truth.currentTarget,
    cortexScope: truth.cortexScope,
    residentServiceAuthority: truth.residentServiceAuthority,
    mcpToolCount: truth.toolCount,
    mcpTools: truth.tools,
    adapterCount: truth.adapterCount,
    adapters: truth.adapters,
    platforms: {
      tier1: platforms.tier1,
      tier2BestEffort: platforms.bestEffort,
    },
    generatedFrom: truth.generatedFrom,
    generatedBy: "scripts/tools/productization/generate-product-truth.mjs",
  };
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

function renderTruthDoc(truth) {
  const toolLines = truth.tools.map((name) => `- \`${name}\``).join("\n");
  const adapterLines = truth.adapters.map((name) => `- \`${name}\``).join("\n");
  const axisLines = truth.axisDefinitions.map(({ id, label, description }) => `| **${label}** | \`${id}\` | ${description} |`).join("\n");
  return [
    "# Membrane product truth (generated)",
    "",
    "This file is generated by `scripts/tools/productization/generate-product-truth.mjs`.",
    "Do not hand-edit; regenerate instead. `--check` fails if this file is stale.",
    "",
    `## MCP tools (${truth.toolCount})`,
    "",
    toolLines,
    "",
    `## Client adapters (${truth.adapterCount})`,
    "",
    adapterLines,
    "",
    "## Six axes",
    "",
    "| Axis | ID | Responsibility |",
    "|---|---|---|",
    axisLines,
    "",
    `Current supported target: **${truth.currentTarget}**. Cortex scope: **${truth.cortexScope}**. Resident service authority: **${truth.residentServiceAuthority}** (Membrane Hub).`,
    "",
  ].join("\n");
}

/** Extract the README's claimed MCP tool count and the tool names it lists. */
function readmeToolClaim(readmeText) {
  const bulletLine = readmeText.split("\n").find((line) => /tools over stdio/i.test(line)) || "";
  const listed = [...bulletLine.matchAll(/`?(membrane_[a-z_]+)`?/g)].map((m) => m[1]);
  const wordMatch = bulletLine.match(/([a-z]+)\s+tools over stdio/i);
  const claimedWordCount = wordMatch ? NUMBER_WORDS[wordMatch[1].toLowerCase()] ?? null : null;
  return { listed, claimedWordCount, bulletLine };
}

/**
 * Pure README-vs-truth evaluation, exported for testing. Returns the list of
 * product-truth failures the README text produces against the source truth:
 * a wrong claimed tool count, a drifted tool list, or a missing generated-doc
 * link. An empty list means the README agrees with source.
 */
export function evaluateReadmeAgainstTruth(readmeText, truth, { docPresent = true } = {}) {
  const failures = [];
  const claim = readmeToolClaim(readmeText);
  if (claim.claimedWordCount !== null && claim.claimedWordCount !== truth.toolCount) {
    failures.push(`README claims ${claim.claimedWordCount} tools but source exposes ${truth.toolCount}`);
  }
  const listedSet = new Set(claim.listed);
  const sourceSet = new Set(truth.tools);
  const missing = truth.tools.filter((name) => !listedSet.has(name));
  const extra = claim.listed.filter((name) => !sourceSet.has(name));
  if (missing.length || extra.length) {
    failures.push(`README tool list drift: missing=[${missing.join(",")}] extra=[${extra.join(",")}]`);
  }
  if (!readmeText.includes("docs/product-truth.md")) {
    failures.push("README does not link the generated product-truth doc (docs/product-truth.md)");
  }
  if (!docPresent) failures.push("README-linked generated doc absent: docs/product-truth.md");
  return failures;
}
/** Run the book-gate product-truth check. Returns { ok, failures: [] }. */
export async function checkProductTruth() {
  const failures = [];
  const truth = await computeProductTruth();
  const platforms = await computePlatformStatus();

  // 1. Generated artifacts must be present and byte-for-byte current.
  const expectedArtifacts = new Map([
    [TRUTH_JSON, renderTruthJson(truth)],
    [TRUTH_DOC, renderTruthDoc(truth)],
    ...renderGeneratedDocs(truth, platforms),
  ]);
  if (existsSync(MANIFEST)) {
    expectedArtifacts.set(MANIFEST, renderManifest(await readFile(MANIFEST, "utf8"), truth, platforms));
  } else {
    failures.push(`missing generated artifact: ${MANIFEST}`);
  }
  for (const [path, expected] of expectedArtifacts) {
    if (!existsSync(path)) {
      failures.push(`missing generated artifact: ${path}`);
      continue;
    }
    const onDisk = await readFile(path, "utf8");
    if (onDisk !== expected) failures.push(`stale generated artifact: ${path} (regenerate with generate-product-truth.mjs)`);
  }

  // 2. README tool-count claim must match the source, and its generated-doc
  //    link must resolve.
  if (!existsSync(README)) {
    failures.push("missing README.md");
  } else {
    const readme = await readFile(README, "utf8");
    failures.push(...evaluateReadmeAgainstTruth(readme, truth, { docPresent: existsSync(TRUTH_DOC) }));
  }

  return { ok: failures.length === 0, failures, truth };
}

/** Write the generated truth artifacts. */
export async function generateProductTruth() {
  const truth = await computeProductTruth();
  const platforms = await computePlatformStatus();
  const artifacts = new Map([
    [TRUTH_JSON, renderTruthJson(truth)],
    [TRUTH_DOC, renderTruthDoc(truth)],
    ...renderGeneratedDocs(truth, platforms),
    [MANIFEST, renderManifest(await readFile(MANIFEST, "utf8"), truth, platforms)],
  ]);
  for (const path of artifacts.keys()) await mkdir(dirname(path), { recursive: true });
  for (const [path, content] of artifacts) await writeFile(path, content);
  return { truth, written: [...artifacts.keys()] };
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const check = process.argv.includes("--check");
  if (check) {
    const result = await checkProductTruth();
    if (!result.ok) {
      for (const failure of result.failures) console.error(`product-truth check FAIL: ${failure}`);
      process.exit(1);
    }
    console.log(`product-truth check OK: ${result.truth.toolCount} tools, ${result.truth.adapterCount} adapters`);
  } else {
    const { truth, written } = await generateProductTruth();
    console.log(`generated product truth: ${truth.toolCount} tools, ${truth.adapterCount} adapters`);
    for (const path of written) console.log(`  wrote ${path}`);
  }
}
