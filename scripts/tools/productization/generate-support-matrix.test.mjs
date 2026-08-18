import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  applyServerJson,
  buildMatrix,
  render,
  renderReadmeBlock,
  spliceReadme,
} from "./generate-support-matrix.mjs";
import { buildReceipt, SCENARIOS } from "../../qualification/run.mjs";

const COMMIT = "a".repeat(40);
const GENERATION = "b".repeat(64);
const ARTIFACT = "c".repeat(64);
const CLIENTS = ["claude", "codex", "cursor", "generic_mcp", "windsurf"];

/**
 * Build a real, fully valid MBR-801 receipt on disk by reusing the
 * production builder (scripts/qualification/run.mjs's buildReceipt), so
 * these tests can never drift from what scripts/qualification/verify-mbr801-evidence.mjs
 * actually accepts.
 */
function writeValidReceipt({ evidenceRoot, platform, commit = COMMIT, releaseGeneration = GENERATION, client = "claude" }) {
  const scenarioResults = SCENARIOS.map((scenario) => ({
    scenario_id: scenario,
    trace_id: `${platform}-${scenario}-trace`,
    gates: { provider: "passed", delivery: "passed", outcome: "passed" },
  }));
  const { receiptPath } = buildReceipt({
    evidenceRoot,
    platform,
    commit,
    releaseGeneration,
    client,
    model: "test-model",
    host: "test-host",
    artifactSha256: ARTIFACT,
    signatureStatus: "passed",
    installedExecution: true,
    scenarioResults,
    benchmark: { status: "complete" },
    generatedAt: "2026-08-08T00:00:00.000Z",
  });
  return receiptPath;
}

function tmpEvidenceRoot() {
  return mkdtempSync(join(tmpdir(), "support-matrix-"));
}

test("no receipts on disk: every platform/client pair is unavailable, never fabricated", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const receiptPaths = {
    macos: join(evidenceRoot, "macos", "receipt.json"),
    windows: join(evidenceRoot, "windows", "receipt.json"),
  };
  const matrix = buildMatrix({ commit: COMMIT, releaseGeneration: GENERATION, clients: CLIENTS, receiptPaths });
  assert.equal(matrix.rows.length, 10);
  assert.ok(matrix.rows.every((row) => row.tier === "unavailable"));
  assert.ok(matrix.rows.every((row) => row.receipt === null));
  assert.ok(matrix.rows.every((row) => /receipt unavailable/.test(row.reason)));
});

test("no current release generation supplied: stays unavailable even with a receipt on disk", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const macosReceipt = writeValidReceipt({ evidenceRoot, platform: "macos" });
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: null,
    clients: CLIENTS,
    receiptPaths: { macos: macosReceipt, windows: join(evidenceRoot, "windows", "receipt.json") },
  });
  assert.ok(matrix.rows.every((row) => row.tier === "unavailable"));
});

test("a real, current, verified macOS receipt qualifies exactly its platform and client", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const macosReceipt = writeValidReceipt({ evidenceRoot, platform: "macos", client: "claude" });
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: GENERATION,
    clients: CLIENTS,
    receiptPaths: { macos: macosReceipt, windows: join(evidenceRoot, "windows", "receipt.json") },
  });

  const macosRows = matrix.rows.filter((row) => row.platform === "macos");
  const windowsRows = matrix.rows.filter((row) => row.platform === "windows");
  assert.equal(macosRows.find((row) => row.client === "claude").tier, "qualified");
  assert.ok(macosRows.filter((row) => row.client !== "claude").every((row) => row.tier === "unavailable"));
  assert.ok(windowsRows.every((row) => row.tier === "unavailable"));

  const qualified = macosRows.find((row) => row.client === "claude");
  assert.equal(qualified.receipt.scenariosPassed, SCENARIOS.length);
  assert.equal(qualified.receipt.scenarioCount, SCENARIOS.length);
  assert.equal(qualified.receipt.commit, COMMIT);
  assert.equal(qualified.receipt.releaseGeneration, GENERATION);
});

test("a receipt for a stale commit never qualifies, even though the file is otherwise valid", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const macosReceipt = writeValidReceipt({ evidenceRoot, platform: "macos", commit: "f".repeat(40) });
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: GENERATION,
    clients: CLIENTS,
    receiptPaths: { macos: macosReceipt, windows: join(evidenceRoot, "windows", "receipt.json") },
  });
  assert.ok(matrix.rows.filter((row) => row.platform === "macos").every((row) => row.tier === "unavailable"));
});

test("a receipt with an incomplete scenario set never qualifies", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const macosReceipt = writeValidReceipt({ evidenceRoot, platform: "macos" });
  const receipt = JSON.parse(readFileSync(macosReceipt, "utf8"));
  receipt.status = "passed"; // an attacker/bug could hand-flip status without fixing the data below
  receipt.scenarios_passed = 9;
  writeFileSync(macosReceipt, JSON.stringify(receipt));
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: GENERATION,
    clients: CLIENTS,
    receiptPaths: { macos: macosReceipt, windows: join(evidenceRoot, "windows", "receipt.json") },
  });
  assert.ok(matrix.rows.filter((row) => row.platform === "macos").every((row) => row.tier === "unavailable"));
});

test("one client's receipt never qualifies a sibling client on the same platform", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const macosReceipt = writeValidReceipt({ evidenceRoot, platform: "macos", client: "codex" });
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: GENERATION,
    clients: CLIENTS,
    receiptPaths: { macos: macosReceipt, windows: join(evidenceRoot, "windows", "receipt.json") },
  });
  const macosRows = matrix.rows.filter((row) => row.platform === "macos");
  assert.equal(macosRows.find((row) => row.client === "codex").tier, "qualified");
  assert.equal(macosRows.find((row) => row.client === "claude").tier, "unavailable");
});

test("render() produces one markdown row per matrix row with tier and reason", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: GENERATION,
    clients: ["claude"],
    receiptPaths: { macos: join(evidenceRoot, "macos", "receipt.json"), windows: join(evidenceRoot, "windows", "receipt.json") },
  });
  const text = render(matrix);
  assert.match(text, /^# Support-tier matrix/);
  assert.match(text, /\| macos \| claude \| installed-path \| unavailable \|/);
  assert.match(text, /\| windows \| claude \| installed-path \| unavailable \|/);
});

test("README splice round-trips through the marker block and rejects a README without markers", () => {
  const matrix = buildMatrix({ commit: COMMIT, releaseGeneration: GENERATION, clients: ["claude"], receiptPaths: {} });
  const block = renderReadmeBlock(matrix);
  assert.match(block, /^<!-- support-matrix:start -->/);
  assert.match(block, /<!-- support-matrix:end -->$/);

  const readme = `# Title\n\n<!-- support-matrix:start -->\nold content\n<!-- support-matrix:end -->\n\n## Next section\n`;
  const spliced = spliceReadme(readme, block);
  assert.ok(spliced.includes(block));
  assert.ok(!spliced.includes("old content"));
  assert.ok(spliced.includes("## Next section"));

  assert.throws(() => spliceReadme("# No markers here", block), /missing the support-matrix markers/);
});

test("applyServerJson only fills a target's platformReceipt when its platform is currently qualified", () => {
  const evidenceRoot = tmpEvidenceRoot();
  const macosReceipt = writeValidReceipt({ evidenceRoot, platform: "macos", client: "claude" });
  const matrix = buildMatrix({
    commit: COMMIT,
    releaseGeneration: GENERATION,
    clients: CLIENTS,
    receiptPaths: { macos: macosReceipt, windows: join(evidenceRoot, "windows", "receipt.json") },
  });

  const serverJson = {
    nativeArtifacts: {
      "darwin-arm64": { platformReceipt: null },
      "darwin-x64": { platformReceipt: null },
      "win32-arm64": { platformReceipt: null },
      "win32-x64": { platformReceipt: null },
    },
  };
  const next = applyServerJson(serverJson, matrix);
  assert.ok(next.nativeArtifacts["darwin-arm64"].platformReceipt);
  assert.ok(next.nativeArtifacts["darwin-x64"].platformReceipt);
  assert.equal(next.nativeArtifacts["darwin-arm64"].platformReceipt.commit, COMMIT);
  assert.equal(next.nativeArtifacts["win32-arm64"].platformReceipt, null);
  assert.equal(next.nativeArtifacts["win32-x64"].platformReceipt, null);
  // original object is untouched
  assert.equal(serverJson.nativeArtifacts["darwin-arm64"].platformReceipt, null);
});

test("applyServerJson never fills a target when no receipt exists anywhere", () => {
  const matrix = buildMatrix({ commit: COMMIT, releaseGeneration: GENERATION, clients: ["claude"], receiptPaths: {} });
  const serverJson = { nativeArtifacts: { "darwin-arm64": { platformReceipt: null }, "win32-arm64": { platformReceipt: null } } };
  const next = applyServerJson(serverJson, matrix);
  assert.equal(next.nativeArtifacts["darwin-arm64"].platformReceipt, null);
  assert.equal(next.nativeArtifacts["win32-arm64"].platformReceipt, null);
});
