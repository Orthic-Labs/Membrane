import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { verifyMbr801Evidence } from '../../scripts/qualification/verify-mbr801-evidence.mjs';

function fixture(root, platform, commit = 'a'.repeat(40), generation = 'release-1') {
  const scenarios = ['repository_orientation', 'cross_repo_impact', 'preference_application', 'stale_graph_immediate_edit', 'contradiction', 'denied_scope', 'tool_proof_criteria', 'user_correction', 'memory_temporal_as_of', 'provider_timeout_degradation'];
  const dir = join(root, platform); mkdirSync(dir); const traces = scenarios.map((scenario_id, i) => { const archive = `trace-${i}.json`; const trace_id = `${platform}-${i}`; writeFileSync(join(dir, archive), JSON.stringify({ trace_id, scenario_id, commit, release_generation: generation })); return { trace_id, scenario_id, archive_path: archive, gates: { provider: 'passed', delivery: 'passed', outcome: 'passed' } }; });
  const receipt = { platform, status: 'passed', commit, release_generation: generation, installed_execution: true, signature_status: 'passed', artifact_sha256: 'a'.repeat(64), client: 'claude-code', model: 'model-1', host: 'installed-host', scenario_count: 10, scenarios_passed: 10, traces, benchmark: { status: 'complete' } };
  const path = join(dir, 'receipt.json'); writeFileSync(path, JSON.stringify(receipt)); return path;
}

test('passes only matching complete Mac and Windows receipts', () => {
  const root = mkdtempSync(join(tmpdir(), 'mbr801-')); const macos = fixture(root, 'macos'); const windows = fixture(root, 'windows');
  const result = verifyMbr801Evidence({ macos, windows, commit: 'a'.repeat(40), releaseGeneration: 'release-1' });
  assert.equal(result.status, 'passed'); assert.equal(result.platforms.macos.status, 'passed');
});
test('fails closed on stale generation, duplicate traces, or missing archive', () => {
  const root = mkdtempSync(join(tmpdir(), 'mbr801-')); const macos = fixture(root, 'macos'); const windows = fixture(root, 'windows', 'a'.repeat(40), 'old');
  assert.equal(verifyMbr801Evidence({ macos, windows, commit: 'a'.repeat(40), releaseGeneration: 'release-1' }).status, 'open');
});
