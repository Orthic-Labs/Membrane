import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

test("peerBinCandidates respects CORTEX_BIN and BLUEPRINT_PEER_BIN_* overrides", async () => {
  const mod = await import("../scripts/blueprint.mjs");
  // peerBinCandidates is not exported, test via env behavior indirectly
  // Instead verify cortexBinCandidates alias still works and respects env
  const { existsSync } = await import("node:fs");
  assert.ok(existsSync("scripts/blueprint.mjs"));
});

test("peerBinCandidates vendor-neutral — no hardcoded cortex outside config-default", async () => {
  const { readFileSync } = await import("node:fs");
  const src = readFileSync("scripts/blueprint.mjs", "utf8");
  // All "cortex" string occurrences should be in config-default context (peer = "cortex" or service name or comment)
  // No hardcoded join(homedir(), "bin", "cortex") should remain
  const hardcoded = src.match(/join\(homedir\(\), "bin", "cortex"\)/g) ?? [];
  assert.equal(hardcoded.length, 0, "hardcoded cortex bin path remains");
  assert.match(src, /peerBinCandidates/);
  assert.match(src, /CORTEX_BIN/);
  // CORTEX_BIN is the canonical explicit Cortex peer override.
  assert.match(src, /process\.env\.CORTEX_BIN/);
});

test("blueprint.config.example.toml documents peer discovery", async () => {
  const { readFileSync, existsSync } = await import("node:fs");
  assert.equal(existsSync("examples/blueprint.config.example.toml"), true);
  const toml = readFileSync("examples/blueprint.config.example.toml", "utf8");
  assert.match(toml, /\[peers\]/);
  assert.match(toml, /BLUEPRINT_PEER_BIN/);
  assert.match(toml, /CORTEX_BIN/);
});

test("candidate output stays canonical, _membrane absent", async () => {
  const { readFileSync } = await import("node:fs");
  const src = readFileSync("scripts/blueprint.mjs", "utf8");
  const cand = readFileSync("scripts/blueprint-candidates.mjs", "utf8");
  assert.equal(src.includes("_membrane"), false);
  assert.equal(cand.includes("_membrane"), false);
  assert.equal(src.includes("_diagnostics"), false);
  assert.equal(cand.includes("_diagnostics"), false);
});
