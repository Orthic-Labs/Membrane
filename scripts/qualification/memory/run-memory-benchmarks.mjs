#!/usr/bin/env node
// MBR-805 CLI harness. Wires benchmark selection and a local dataset path to
// the runner in benchmarks/memory/runner.mjs. It never downloads a dataset
// and never invokes the Cortex engine; absent a dataset root (or an executor,
// which this CLI does not supply) it reports an explicit degrade per
// benchmark rather than fabricating a result.
import { runMemoryBenchmark } from '../../../tests/benchmarks/memory/runner.mjs';
import { BENCHMARKS } from '../../../tests/benchmarks/memory/contracts.mjs';

export function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--benchmark') out.benchmark = argv[(i += 1)];
    else if (arg === '--dataset-root') out.datasetRoot = argv[(i += 1)];
  }
  return out;
}

export async function runReport(argv) {
  const { benchmark, datasetRoot } = parseArgs(argv);
  if (benchmark && !BENCHMARKS.includes(benchmark)) throw new Error(`unsupported benchmark: ${benchmark}`);
  const targets = benchmark ? [benchmark] : BENCHMARKS;
  const results = [];
  for (const target of targets) results.push(await runMemoryBenchmark({ benchmark: target, datasetRoot }));
  return { schema: 'membrane.memory-benchmark-run-report.v1', results };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  runReport(process.argv.slice(2))
    .then(report => {
      process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    })
    .catch(error => {
      process.stderr.write(`${error.message}\n`);
      process.exitCode = 2;
    });
}
