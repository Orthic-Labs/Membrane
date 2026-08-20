#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const REQUIRED = 10;
const SCENARIOS = ['repository_orientation', 'cross_repo_impact', 'preference_application', 'stale_graph_immediate_edit', 'contradiction', 'denied_scope', 'tool_proof_criteria', 'user_correction', 'memory_temporal_as_of', 'provider_timeout_degradation'];
const fail = (reason) => ({ status: 'open', reason });
const read = (path) => { try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; } };

function verifyReceipt(path, platform, expected) {
  if (!path || !existsSync(path)) return fail(`${platform} receipt unavailable`);
  const value = read(path);
  if (!value) return fail(`${platform} receipt is not valid JSON`);
  if (value.platform !== platform) return fail(`${platform} receipt platform mismatch`);
  if (value.status !== 'passed') return fail(`${platform} receipt is not passed`);
  if (value.commit !== expected.commit || value.release_generation !== expected.releaseGeneration) return fail(`${platform} identity or release generation mismatch`);
  if (value.installed_execution !== true || value.signature_status !== 'passed' || !/^[0-9a-f]{64}$/.test(value.artifact_sha256 || '')) return fail(`${platform} signed installed execution is not proven`);
  if (![value.client, value.model, value.host].every(item => typeof item === 'string' && item.trim())) return fail(`${platform} real client, model, or host identity is missing`);
  if (value.scenario_count !== REQUIRED || value.scenarios_passed !== REQUIRED || !Array.isArray(value.traces) || value.traces.length !== REQUIRED) return fail(`${platform} canonical scenario set is incomplete`);
  const ids = value.traces.map(trace => trace?.trace_id);
  if (ids.some(id => typeof id !== 'string' || !id) || new Set(ids).size !== REQUIRED) return fail(`${platform} trace IDs are missing or not unique`);
  if (new Set(value.traces.map(trace => trace?.scenario_id)).size !== REQUIRED || SCENARIOS.some(name => !value.traces.some(trace => trace.scenario_id === name))) return fail(`${platform} canonical scenarios are missing or duplicated`);
  if (value.traces.some(trace => !['provider', 'delivery', 'outcome'].every(gate => trace.gates?.[gate] === 'passed'))) return fail(`${platform} provider, delivery, or outcome evidence is incomplete`);
  if (value.benchmark?.status !== 'complete') return fail(`${platform} benchmark is incomplete`);
  if (value.traces.some(trace => { const archive = typeof trace.archive_path === 'string' && resolve(path, '..', trace.archive_path); const saved = archive && existsSync(archive) && read(archive); return !saved || saved.trace_id !== trace.trace_id || saved.scenario_id !== trace.scenario_id || saved.commit !== expected.commit || saved.release_generation !== expected.releaseGeneration; })) return fail(`${platform} archived receipt identity is unavailable or mismatched`);
  return { status: 'passed', reason: 'current installed receipt, scenarios, traces, benchmark, and archives verified' };
}

export function verifyMbr801Evidence(input) {
  const expected = { commit: input?.commit, releaseGeneration: input?.releaseGeneration };
  if (!/^[0-9a-f]{40}$/.test(expected.commit || '') || typeof expected.releaseGeneration !== 'string' || !expected.releaseGeneration) {
    return { schema: 'membrane.mbr801-evidence-validation.v1', status: 'open', reason: 'expected current commit and release generation are required', platforms: { macos: fail('not checked'), windows: fail('not checked') } };
  }
  const platforms = { macos: verifyReceipt(input.macos, 'macos', expected), windows: verifyReceipt(input.windows, 'windows', expected) };
  return { schema: 'membrane.mbr801-evidence-validation.v1', status: platforms.macos.status === 'passed' && platforms.windows.status === 'passed' ? 'passed' : 'open', platforms };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const args = process.argv.slice(2); const value = name => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
  const summary = verifyMbr801Evidence({ macos: value('--macos'), windows: value('--windows'), commit: value('--commit'), releaseGeneration: value('--release-generation') });
  process.stdout.write(`${JSON.stringify(summary)}\n`); process.exitCode = summary.status === 'passed' ? 0 : 2;
}
