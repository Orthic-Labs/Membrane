#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

const HASH = /^[0-9a-f]{64}$/;
const fail = reason => ({ status: 'open', reason });
const load = input => typeof input === 'string' ? (existsSync(input) ? { value: JSON.parse(readFileSync(input, 'utf8')), base: dirname(resolve(input)) } : {}) : { value: input, base: process.cwd() };
const required = (o, keys) => keys.every(k => o && o[k] !== undefined && o[k] !== null);
const fileMatches = (base, path, digest) => { const target = typeof path === 'string' && resolve(base, path); return target && existsSync(target) && createHash('sha256').update(readFileSync(target)).digest('hex') === digest; };

export function verifyWholeTaskBenchmark(input) {
  let value;
  let base;
  try { ({ value, base } = load(input)); } catch { return fail('receipt is not valid JSON'); }
  if (!value || value.schema !== 'orthic.mbr804-whole-task.v1') return fail('schema identity missing');
  if (value.status !== 'complete') return fail('real current results are required; source-ready is not PASS');
  if (!value.run?.measured || !required(value.run, ['run_id', 'started_at', 'ended_at'])) return fail('run is not measured and timestamped');
  const task = value.task;
  if (!task || task.task_id !== 'MBR-804' || !required(task.release, ['commit', 'generation', 'client', 'service']) || !/^[0-9a-f]{40}$/.test(task.release.commit)) return fail('exact release identity missing');
  if (!required(task.corpus, ['id', 'version', 'sha256', 'cases']) || !HASH.test(task.corpus.sha256) || task.corpus.cases < 1) return fail('exact corpus identity missing');
  if (!Array.isArray(task.models) || !task.models.length || !task.hardware?.macos || !task.hardware?.windows) return fail('exact models or hardware missing');
  if (!task.controls || !required(task.controls, ['unauthorized_context', 'stale_context', 'missed_authoritative', 'cache_policy'])) return fail('control definitions missing');
  if (!value.bakeoff?.immutable || !required(value.bakeoff, ['config', 'config_sha256', 'input', 'input_sha256', 'receipt', 'receipt_sha256']) || !['config', 'input', 'receipt'].every(key => HASH.test(value.bakeoff[`${key}_sha256`]) && fileMatches(base, value.bakeoff[key], value.bakeoff[`${key}_sha256`])) || !Array.isArray(value.bakeoff.arms) || value.bakeoff.arms.length < 2) return fail('immutable bakeoff files, hashes, or arms missing');
  for (const platform of ['macos', 'windows']) { const h = value.hosts?.[platform]; if (!h || h.status !== 'measured' || !required(h, ['receipt', 'receipt_sha256', 'release', 'hardware']) || !HASH.test(h.receipt_sha256) || !fileMatches(base, h.receipt, h.receipt_sha256) || h.release !== task.release.generation) return fail(`${platform} host receipt missing, stale, or mismatched`); }
  if (!required(value.metrics, ['task_success', 'context_errors', 'latency_ms', 'tokens', 'cache', 'cost'])) return fail('required measured metrics missing');
  if (Object.values(value.metrics).some(metric => metric.measured !== true || typeof metric.value !== 'number' || typeof metric.unit !== 'string' || !metric.unit)) return fail('metrics must include measured numeric values and units');
  if (!Array.isArray(value.failures)) return fail('failure list missing');
  if (!value.publication || !['not-published', 'internal-only', 'published'].includes(value.publication.disposition) || typeof value.publication.approved !== 'boolean' || (value.publication.disposition === 'published' && !value.publication.approved)) return fail('publication disposition missing or unapproved');
  return { status: 'passed', reason: 'current measured whole-task results, controls, bakeoff hashes, and both host receipts verified' };
}

if (import.meta.url === `file://${process.argv[1]}`) { const p = process.argv[2]; const out = verifyWholeTaskBenchmark(p); process.stdout.write(`${JSON.stringify(out)}\n`); process.exitCode = out.status === 'passed' ? 0 : 2; }
