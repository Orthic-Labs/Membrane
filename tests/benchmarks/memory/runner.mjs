import { existsSync } from 'node:fs';
import { BENCHMARKS, COMPONENT } from './contracts.mjs';
import { verifyMemoryBenchmark } from '../../../scripts/qualification/verify-memory-benchmark.mjs';

// This module never downloads a dataset and never invokes the Cortex engine
// itself. It only wires: (1) local dataset presence, (2) a caller-supplied
// executor that performs the actual benchmark run, and (3) the existing
// MBR-805 contract/verifier so a run can never report a Membrane/managed
// score or an unmeasured value as measured. Any missing precondition
// degrades explicitly instead of fabricating a result.

function degrade(benchmark, reason, extra = {}) {
  return { schema: 'membrane.memory-benchmark-run.v1', benchmark, componentUnderTest: COMPONENT, status: 'degraded', reason, ...extra };
}

/**
 * @param {object} options
 * @param {'LoCoMo'|'LongMemEval'|'BEAM'} options.benchmark
 * @param {string} [options.datasetRoot] local path to a benchmark dataset or fixture directory; never fetched or downloaded by this function
 * @param {(ctx: { benchmark: string, datasetRoot: string }) => Promise<object>|object} [options.execute] caller-supplied executor that performs the actual run and returns a raw benchmark payload; this module does not implement one
 * @returns {Promise<object>} a result with `status` of `degraded`, `invalid`, or `ran`; only `ran` carries verified metrics
 */
export async function runMemoryBenchmark({ benchmark, datasetRoot, execute } = {}) {
  if (!BENCHMARKS.includes(benchmark)) throw new Error(`unsupported benchmark: ${benchmark}`);
  if (!datasetRoot) return degrade(benchmark, 'dataset-root-not-configured');
  if (!existsSync(datasetRoot)) return degrade(benchmark, 'dataset-not-found', { datasetRoot });
  if (typeof execute !== 'function') return degrade(benchmark, 'executor-not-configured', { datasetRoot });

  const raw = await execute({ benchmark, datasetRoot });
  let verification;
  try {
    verification = verifyMemoryBenchmark({ benchmark, ...raw });
  } catch (error) {
    return { schema: 'membrane.memory-benchmark-run.v1', benchmark, componentUnderTest: COMPONENT, status: 'invalid', reason: error.message, datasetRoot };
  }
  if (verification.status !== 'passed') {
    return { schema: 'membrane.memory-benchmark-run.v1', benchmark, componentUnderTest: COMPONENT, status: 'invalid', reason: verification.reason, datasetRoot };
  }
  return { ...verification, schema: 'membrane.memory-benchmark-run.v1', status: 'ran', datasetRoot };
}

/**
 * Runs every supported benchmark and returns one report entry per benchmark.
 * Benchmarks with no dataset/executor configured degrade individually; one
 * benchmark's degrade or invalid result never blocks the others from running.
 */
export async function runAllMemoryBenchmarks({ datasetRoot, execute } = {}) {
  const results = [];
  for (const benchmark of BENCHMARKS) {
    results.push(await runMemoryBenchmark({ benchmark, datasetRoot, execute }));
  }
  return { schema: 'membrane.memory-benchmark-run-report.v1', results };
}
