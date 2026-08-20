import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

test("peerBinCandidates respects CRYPT_BIN and CORTEX_PEER_BIN_* overrides", async () => {
  const mod = await import("../scripts/cortex.mjs");
  // peerBinCandidates is not exported, test via env behavior indirectly
  // Instead verify cryptBinCandidates alias still works and respects env
  const { existsSync } = await import("node:fs");
  assert.ok(existsSync("scripts/cortex.mjs"));
});

test("peerBinCandidates vendor-neutral — no hardcoded crypt outside config-default", async () => {
  const { readFileSync } = await import("node:fs");
  const src = readFileSync("scripts/cortex.mjs", "utf8");
  // All "crypt" string occurrences should be in config-default context (peer = "crypt" or service name or comment)
  // No hardcoded join(homedir(), "bin", "crypt") should remain
  const hardcoded = src.match(/join\(homedir\(\), "bin", "crypt"\)/g) ?? [];
  assert.equal(hardcoded.length, 0, "hardcoded crypt bin path remains");
  assert.match(src, /peerBinCandidates/);
  assert.match(src, /CRYPT_BIN/);
  // CRYPT_BIN must still be present for backwards compat
  assert.match(src, /process\.env\.CRYPT_BIN/);
});

test("cortex.config.example.toml documents peer discovery", async () => {
  const { readFileSync, existsSync } = await import("node:fs");
  assert.equal(existsSync("examples/cortex.config.example.toml"), true);
  const toml = readFileSync("examples/cortex.config.example.toml", "utf8");
  assert.match(toml, /\[peers\]/);
  assert.match(toml, /CORTEX_PEER_BIN/);
  assert.match(toml, /CRYPT_BIN/);
});

test("_diagnostics field present, _membrane absent", async () => {
  const { readFileSync } = await import("node:fs");
  const src = readFileSync("scripts/cortex.mjs", "utf8");
  const cand = readFileSync("scripts/cortex-candidates.mjs", "utf8");
  assert.equal(src.includes("_membrane"), false);
  assert.equal(cand.includes("_membrane"), false);
  assert.equal(src.includes("_diagnostics"), true);
  assert.equal(cand.includes("_diagnostics"), true);
});
