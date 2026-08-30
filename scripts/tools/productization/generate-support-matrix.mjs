#!/usr/bin/env node
// scripts/tools/productization/generate-support-matrix.mjs — MBR-808.
//
// Derives the published support-tier matrix from real MBR-801 installed-path
// conformance receipts — never from prose or a hand-maintained table. The
// generator reuses the existing MBR-801 validator
// (scripts/qualification/verify-mbr801-evidence.mjs) rather than
// re-implementing conformance logic, and writes the SAME generated matrix
// into every surface this task owns: docs/product/support/matrix.md & matrix.json,
// a generated block in README.md, and the per-target platformReceipt fields
// in server.json (the MCP Registry server descriptor). A platform/client pair
// with no current, verified receipt renders "unavailable" — never "qualified"
// and never silently omitted.
//
// Client universe: read (never written) from docs/reference/clients/support-matrix.v1.json,
// the MBR-206 client registry — this generator adds no second, competing
// client list.
//
// Run:  node scripts/tools/productization/generate-support-matrix.mjs [--commit <sha>] [--release-generation <hex>]
// Book-gate real use additionally requires a real MBR-801 receipt on disk at
// docs/evidence/qualification/mbr801/<platform>/receipt.json for the platform to
// ever show "qualified".

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verifyMbr801Evidence } from "../../qualification/verify-mbr801-evidence.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const PLATFORMS = ["macos", "windows"];
const README_START = "<!-- support-matrix:start -->";
const README_END = "<!-- support-matrix:end -->";
const PLATFORM_TARGETS = { macos: ["darwin-arm64", "darwin-x64"], windows: ["win32-arm64", "win32-x64"] };

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

/** The canonical, generated (MBR-206) client id list. Read-only dependency. */
export function clientUniverse(repoRoot) {
  const matrix = readJson(join(repoRoot, "docs", "reference", "clients", "support-matrix.v1.json"));
  return [...new Set(matrix.cells.map((cell) => cell.client))].sort();
}

/**
 * Verify one platform's MBR-801 receipt through the real, existing validator.
 * Never re-implements the pass/fail rules: a receipt this function treats as
 * "passed" is exactly one scripts/qualification/verify-mbr801-evidence.mjs
 * also treats as "passed".
 */
function platformConformance(platform, receiptPath, expected) {
  const verification = verifyMbr801Evidence({
    macos: platform === "macos" ? receiptPath : undefined,
    windows: platform === "windows" ? receiptPath : undefined,
    commit: expected.commit,
    releaseGeneration: expected.releaseGeneration,
  });
  // The current qualification authority may intentionally expose only the
  // supported target (macOS). Keep this derived matrix total and honest for
  // retained, unavailable platform rows rather than dereferencing a missing
  // authority result.
  const result = verification.platforms[platform] ?? { status: "open", reason: `${platform} receipt unavailable` };
  if (result.status !== "passed") {
    const reason = result.reason === "not checked"
      ? "no current commit/release-generation to verify against (release evidence not yet published)"
      : result.reason;
    return { status: "unavailable", reason, receipt: null };
  }
  if (!receiptPath || !existsSync(receiptPath)) return { status: "unavailable", reason: `${platform} receipt unavailable`, receipt: null };
  return { status: "passed", reason: result.reason, receipt: readJson(receiptPath) };
}

/**
 * Pure matrix builder — no disk writes. `receiptPaths` maps platform -> path
 * to a candidate docs/evidence/qualification/mbr801/<platform>/receipt.json.
 * Exported for deterministic testing.
 */
export function buildMatrix({ commit, releaseGeneration, clients, receiptPaths }) {
  const expected = { commit: commit ?? null, releaseGeneration: releaseGeneration ?? null };
  const conformance = Object.fromEntries(
    PLATFORMS.map((platform) => [platform, platformConformance(platform, receiptPaths?.[platform], expected)]),
  );

  const rows = PLATFORMS.flatMap((platform) => {
    const platformResult = conformance[platform];
    const qualifiedClient = platformResult.status === "passed" ? platformResult.receipt.client : null;
    return clients.map((client) => {
      const tier = qualifiedClient === client ? "qualified" : "unavailable";
      let reason;
      if (tier === "qualified") {
        reason = platformResult.reason;
      } else if (platformResult.status === "passed") {
        reason = `current installed-path receipt on ${platform} names client "${qualifiedClient}"; no current receipt names "${client}"`;
      } else {
        reason = platformResult.reason;
      }
      return {
        platform,
        client,
        feature: "installed-path",
        tier,
        reason,
        receipt: tier === "qualified"
          ? {
              schema: platformResult.receipt.schema,
              platform,
              client,
              commit: platformResult.receipt.commit,
              releaseGeneration: platformResult.receipt.release_generation,
              scenariosPassed: platformResult.receipt.scenarios_passed,
              scenarioCount: platformResult.receipt.scenario_count,
            }
          : null,
      };
    });
  });

  return {
    schema: "membrane.support-tier-matrix.v1",
    generatedFrom: {
      commit: expected.commit,
      releaseGeneration: expected.releaseGeneration,
      source: "scripts/qualification/run.mjs + scripts/qualification/verify-mbr801-evidence.mjs",
      receiptDirectory: "docs/evidence/qualification/mbr801",
    },
    rows,
  };
}

export function render(matrix) {
  const lines = [
    "# Support-tier matrix",
    "",
    "Generated from current MBR-801 installed-path ten-scenario conformance receipts",
    "(`scripts/qualification/run.mjs`, verified by `scripts/qualification/verify-mbr801-evidence.mjs`).",
    "Do not hand-edit this file. `unavailable` never means unsupported, healthy, or in",
    "progress — it means no current, verified conformance receipt exists for that",
    "platform/client pair.",
    "",
    "| Platform | Client | Feature | Tier | Reason |",
    "| --- | --- | --- | --- | --- |",
  ];
  for (const row of matrix.rows) lines.push(`| ${row.platform} | ${row.client} | ${row.feature} | ${row.tier} | ${row.reason} |`);
  return `${lines.join("\n")}\n`;
}

/** Short, marker-delimited summary block spliced into README.md. */
export function renderReadmeBlock(matrix) {
  const qualified = matrix.rows.filter((row) => row.tier === "qualified").length;
  return [
    README_START,
    "## Support tier matrix",
    "",
    `Generated from current MBR-801 installed-path conformance receipts — ${qualified} of ${matrix.rows.length}`,
    "platform/client pairs currently qualified. Full table, tiers, and reasons:",
    "[docs/product/support/matrix.md](docs/product/support/matrix.md) (also machine-readable at `docs/product/support/matrix.json`).",
    "This block, the JSON/MD matrix, and `server.json`'s per-target `platformReceipt`",
    "fields are all written by `node scripts/tools/productization/generate-support-matrix.mjs`",
    "from the same receipts; none of them is hand-maintained.",
    README_END,
  ].join("\n");
}

/** Replace the marker-delimited block in README text. Throws if markers are absent. */
export function spliceReadme(readmeText, block) {
  const startIndex = readmeText.indexOf(README_START);
  const endIndex = readmeText.indexOf(README_END);
  if (startIndex === -1 || endIndex === -1) throw new Error("README.md is missing the support-matrix markers");
  return `${readmeText.slice(0, startIndex)}${block}${readmeText.slice(endIndex + README_END.length)}`;
}

/**
 * Apply the same matrix to server.json's per-target platformReceipt fields —
 * the MCP Registry server descriptor. A target's receipt is non-null only
 * when its platform has a currently verified MBR-801 receipt; the specific
 * qualifying client does not change the platform-level artifact receipt.
 */
export function applyServerJson(serverJson, matrix) {
  const next = structuredClone(serverJson);
  for (const [platform, targets] of Object.entries(PLATFORM_TARGETS)) {
    const qualifiedRow = matrix.rows.find((row) => row.platform === platform && row.tier === "qualified");
    const receipt = qualifiedRow
      ? {
          schema: qualifiedRow.receipt.schema,
          commit: qualifiedRow.receipt.commit,
          releaseGeneration: qualifiedRow.receipt.releaseGeneration,
          scenariosPassed: qualifiedRow.receipt.scenariosPassed,
          scenarioCount: qualifiedRow.receipt.scenarioCount,
        }
      : null;
    for (const target of targets) {
      if (next.nativeArtifacts?.[target]) next.nativeArtifacts[target].platformReceipt = receipt;
    }
  }
  return next;
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function safeGitCommit(repoRoot) {
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

/** Full generation: computes the matrix and writes every owned surface. */
export function generate({ repoRoot = root, commit, releaseGeneration, receiptPaths } = {}) {
  const clients = clientUniverse(repoRoot);
  const currentCommit = commit ?? safeGitCommit(repoRoot);
  const paths = receiptPaths ?? {
    macos: join(repoRoot, "docs", "evidence", "qualification", "mbr801", "macos", "receipt.json"),
    windows: join(repoRoot, "docs", "evidence", "qualification", "mbr801", "windows", "receipt.json"),
  };
  const matrix = buildMatrix({ commit: currentCommit, releaseGeneration: releaseGeneration ?? null, clients, receiptPaths: paths });

  writeFileSync(join(repoRoot, "docs", "product", "support", "matrix.json"), `${JSON.stringify(matrix, null, 2)}\n`);
  writeFileSync(join(repoRoot, "docs", "product", "support", "matrix.md"), render(matrix));

  const readmePath = join(repoRoot, "README.md");
  writeFileSync(readmePath, spliceReadme(readFileSync(readmePath, "utf8"), renderReadmeBlock(matrix)));

  const serverJsonPath = join(repoRoot, "server.json");
  const currentServerJson = readJson(serverJsonPath);
  const nextServerJson = applyServerJson(currentServerJson, matrix);
  // Only rewrite when a receipt actually changes a platformReceipt value —
  // avoids gratuitous reformatting of a file this generator does not own the
  // style of when there is no semantic change to report.
  if (canonicalJson(nextServerJson) !== canonicalJson(currentServerJson)) {
    writeFileSync(serverJsonPath, `${JSON.stringify(nextServerJson, null, 2)}\n`);
  }

  return matrix;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const value = (name) => {
    const index = args.indexOf(name);
    return index < 0 ? undefined : args[index + 1];
  };
  generate({ commit: value("--commit"), releaseGeneration: value("--release-generation") });
}
