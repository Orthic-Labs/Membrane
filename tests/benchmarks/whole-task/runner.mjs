// MBR-804 — whole-task benchmark runner.
//
// Wires: (1) a corpus (the fixture at benchmarks/whole-task/corpus.fixture.json
// by default, or a real corpus supplied by the caller), (2) the commit-reveal
// holdout protocol in ./holdout.mjs, (3) the immutable, un-rerun vector
// bakeoff reference in ./bakeoff-reference.mjs, and (4) a caller-supplied
// `execute` function that performs the actual per-case run — this module
// never invokes a client, model, or the Cortex engine itself. Every produced
// receipt is validated by the existing, unmodified
// scripts/qualification/verify-whole-task-benchmark.mjs before being reported
// as measured evidence.
//
// A corpus with `fixture: true` can never produce status "complete": the
// guard below forces "source-ready" for fixture corpora regardless of what
// the caller-supplied executor returns, so a fixture self-test can never be
// mistaken for, or reported as, a real measured product benchmark. This
// mirrors docs/evaluation/whole-task-benchmark.md: "status: source-ready
// describes this protocol only and can never produce PASS."

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { hostname } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { splitCorpus, verifySplit } from './holdout.mjs';
import { BAKEOFF_REFERENCE, verifyBakeoffReference } from './bakeoff-reference.mjs';
import { verifyWholeTaskBenchmark } from '../../../scripts/qualification/verify-whole-task-benchmark.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = resolve(HERE, '../../..');
export const DEFAULT_CORPUS_PATH = resolve(HERE, 'corpus.fixture.json');

// A Mac receipt for THIS harness (mirrors MBR-801's
// docs/evidence/qualification/mbr801/macos/receipt.json pattern): real
// hostname/arch, a caller-supplied release generation, and a `measured`
// flag the caller sets only after cases actually ran on that machine. This
// never fabricates measurement — it records identity for a platform the
// caller asserts (via `measured`) it actually exercised.
export function writeHostReceipt({ evidenceRoot, platform, releaseGeneration, measured = true, now = () => new Date().toISOString() }) {
  if (platform !== 'macos') throw new Error('Mac-only benchmark accepts platform macos');
  if (typeof releaseGeneration !== 'string' || !releaseGeneration) throw new Error('releaseGeneration is required');
  const receipt = {
    schema: 'membrane.mbr804-host-receipt.v1',
    platform,
    release: releaseGeneration,
    hardware: `${platform}-${process.arch}-${hostname() || 'unknown-host'}`,
    status: measured ? 'measured' : 'missing',
    generated_at: now(),
  };
  const path = resolve(evidenceRoot, platform, 'receipt.json');
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(receipt, null, 2)}\n`);
  return { path, receipt };
}

function loadHostReceipt(path) {
  if (!path || !existsSync(path)) return null;
  const bytes = readFileSync(path);
  try {
    return { path, content: JSON.parse(bytes.toString('utf8')), sha256: createHash('sha256').update(bytes).digest('hex') };
  } catch {
    return null;
  }
}

function degrade(reason, extra = {}) {
  return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'degraded', reason, ...extra };
}

function loadCorpus(corpusPath) {
  if (!corpusPath) return null;
  if (!existsSync(corpusPath)) return null;
  return JSON.parse(readFileSync(corpusPath, 'utf8'));
}

function defaultGitCommit(workspaceRoot) {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: workspaceRoot, encoding: 'utf8' }).trim();
  } catch {
    return null;
  }
}

function summarizeMetric(values, unit) {
  const finite = values.filter((v) => typeof v === 'number' && Number.isFinite(v));
  const value = finite.length ? finite.reduce((a, b) => a + b, 0) / finite.length : 0;
  return { measured: true, value, unit, n: finite.length };
}

/**
 * @param {object} options
 * @param {string} [options.corpusPath] path to a corpus JSON file (defaults to the fixture corpus)
 * @param {string} options.salt the holdout salt (must match holdoutCommitment)
 * @param {string} options.holdoutCommitment sha256(salt), published before the run
 * @param {number} [options.holdoutFraction] fraction of the corpus withheld as holdout (default 0.3)
 * @param {{commit?:string, generation:string, client:string, service:string}} [options.release]
 * @param {string[]} [options.models]
 * @param {{macos:string}} [options.hardware]
 * @param {(ctx:{ case:object, group:'visible'|'holdout' }) => Promise<object>|object} [options.execute]
 *   caller-supplied per-case executor; returns { success, contextError?:{unauthorized?,stale?,missedAuthoritative?}, latencyMs, tokens, cacheHit, costUsd }
 * @param {{macos?:string}} [options.hostReceiptPaths] path to a real, already-written Mac receipt (see writeHostReceipt)
 * @param {{disposition:'not-published'|'internal-only'|'published', approved:boolean}} [options.publication]
 * @param {() => string} [options.now]
 * @param {string} [options.workspaceRoot]
 */
export async function runWholeTaskBenchmark(options = {}) {
  const {
    corpusPath = DEFAULT_CORPUS_PATH,
    salt,
    holdoutCommitment,
    holdoutFraction = 0.3,
    release = {},
    models = [],
    hardware = {},
    controls = { unauthorized_context: true, stale_context: true, missed_authoritative: true, cache_policy: 'bounded' },
    execute,
    hostReceiptPaths = {},
    publication = { disposition: 'not-published', approved: false },
    now = () => new Date().toISOString(),
    workspaceRoot = WORKSPACE_ROOT,
    gitCommit = () => defaultGitCommit(workspaceRoot),
  } = options;

  const corpus = loadCorpus(corpusPath);
  if (!corpus) return degrade('corpus-not-found', { corpusPath });
  if (!Array.isArray(corpus.cases) || corpus.cases.length === 0) return degrade('corpus-empty', { corpusPath });
  if (typeof salt !== 'string' || typeof holdoutCommitment !== 'string') return degrade('holdout-not-configured');
  if (typeof execute !== 'function') return degrade('executor-not-configured', { corpusPath });

  const isFixtureCorpusEarly = corpus.fixture === true;
  const macHost = loadHostReceipt(hostReceiptPaths.macos);
  if (!isFixtureCorpusEarly && !macHost) {
    return degrade('host-receipts-not-configured', { missing: ['macos'] });
  }

  const caseIds = corpus.cases.map((c) => c.id);
  let split;
  try {
    split = splitCorpus({ caseIds, salt, holdoutFraction });
  } catch (error) {
    return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'invalid', reason: `holdout split failed: ${error.message}` };
  }
  if (split.saltCommitment !== holdoutCommitment) {
    return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'invalid', reason: 'holdout salt does not match the pre-committed commitment; a split cannot be trusted if the salt used to compute it does not match what was published before the run' };
  }
  const splitCheck = verifySplit({ caseIds, salt, holdoutCommitment, claimed: split });
  if (splitCheck.status !== 'verified') return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'invalid', reason: `holdout split failed self-verification: ${splitCheck.reason}` };

  const bakeoff = verifyBakeoffReference({ workspaceRoot });
  if (bakeoff.status !== 'verified') return degrade('bakeoff-reference-unavailable', { bakeoff });

  const startedAt = now();
  const perCase = [];
  const failures = [];
  for (const item of corpus.cases) {
    const group = split.assignment[item.id];
    let result;
    try {
      result = await execute({ case: item, group });
    } catch (error) {
      result = { success: false, error: error.message };
    }
    perCase.push({ id: item.id, category: item.category, group, ...result });
    if (!result || result.success !== true) {
      failures.push({ id: item.id, category: item.category, group, reason: result?.error || 'case did not report success:true' });
    }
  }
  const endedAt = now();

  const holdoutCases = perCase.filter((c) => c.group === 'holdout');
  const visibleCases = perCase.filter((c) => c.group === 'visible');
  const successRate = (rows) => (rows.length ? rows.filter((r) => r.success === true).length / rows.length : 0);
  const contextErrorCount = perCase.reduce((total, c) => total + ['unauthorized', 'stale', 'missedAuthoritative'].filter((k) => c.contextError?.[k]).length, 0);

  const metrics = {
    task_success: { measured: true, value: successRate(holdoutCases), unit: 'ratio', visible: successRate(visibleCases), holdout: successRate(holdoutCases) },
    context_errors: { measured: true, value: contextErrorCount, unit: 'count' },
    latency_ms: summarizeMetric(perCase.map((c) => c.latencyMs), 'ms'),
    tokens: summarizeMetric(perCase.map((c) => c.tokens), 'count'),
    cache: { measured: true, value: perCase.length ? perCase.filter((c) => c.cacheHit === true).length / perCase.length : 0, unit: 'ratio' },
    cost: summarizeMetric(perCase.map((c) => c.costUsd), 'usd'),
  };

  const commit = release.commit || gitCommit();
  const isFixtureCorpus = corpus.fixture === true;

  const receipt = {
    schema: 'membrane.mbr804-whole-task.v1',
    status: isFixtureCorpus ? 'source-ready' : 'complete',
    run: { run_id: `${corpus.id}:${startedAt}`, started_at: startedAt, ended_at: endedAt, measured: true },
    task: {
      task_id: 'MBR-804',
      release: { commit: commit || '0'.repeat(40), generation: release.generation || 'unset', client: release.client || 'unset', service: release.service || 'unset' },
      corpus: { id: corpus.id, version: corpus.version, sha256: sha256OfCorpus(corpusPath), cases: corpus.cases.length, fixture: isFixtureCorpus },
      models: models.length ? models : ['unset'],
      hardware: { macos: hardware.macos || 'unset' },
      controls,
    },
    holdout: { protocol: 'commit-reveal-sha256-bucket', saltCommitment: split.saltCommitment, holdoutFraction: split.holdoutFraction, splitSha256: split.splitSha256, visibleCount: split.visible.length, holdoutCount: split.holdout.length },
    bakeoff: {
      config: bakeoffPath('config', workspaceRoot), config_sha256: bakeoff.checks[0].actual,
      input: bakeoffPath('input', workspaceRoot), input_sha256: bakeoff.checks[1].actual,
      receipt: bakeoffPath('receipt', workspaceRoot), receipt_sha256: bakeoff.checks[2].actual,
      immutable: true, arms: bakeoff.arms, declared: bakeoff.declared,
      // Existing Mac bakeoff receipt, consumed read-only — never rerun.
      hostReceiptsConsumed: {
        macos: { path: bakeoffPath('hosts.macos', workspaceRoot), sha256: bakeoff.checks[3].actual },
      },
    },
    hosts: {
      macos: macHost ? { receipt: macHost.path, receipt_sha256: macHost.sha256, release: macHost.content.release, hardware: macHost.content.hardware, status: macHost.content.status } : { receipt: 'unset', receipt_sha256: '0'.repeat(64), release: 'unset', hardware: 'unset', status: 'missing' },
    },
    metrics,
    failures,
    publication,
  };

  if (isFixtureCorpus) {
    return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'source-ready', reason: 'fixture corpus: protocol self-test only, cannot be reported as a measured product benchmark', receipt, perCase };
  }

  const verification = verifyWholeTaskBenchmark(receipt);
  if (verification.status !== 'passed') {
    return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'invalid', reason: verification.reason, receipt, perCase };
  }
  return { schema: 'membrane.whole-task-benchmark-run.v1', status: 'ran', receipt, perCase, verification };
}

function sha256OfCorpus(corpusPath) {
  return createHash('sha256').update(readFileSync(corpusPath)).digest('hex');
}

// Resolved to absolute paths (from the single source of truth in
// bakeoff-reference.mjs) so the schema verifier's fileMatches() check always
// finds the real, immutable artifact regardless of caller cwd.
function bakeoffPath(key, workspaceRoot) {
  const lookup = {
    config: BAKEOFF_REFERENCE.artifacts.config.path,
    input: BAKEOFF_REFERENCE.artifacts.input.path,
    receipt: BAKEOFF_REFERENCE.artifacts.receipt.path,
    'hosts.macos': BAKEOFF_REFERENCE.hosts.macos.path,
  };
  return resolve(workspaceRoot, lookup[key]);
}
