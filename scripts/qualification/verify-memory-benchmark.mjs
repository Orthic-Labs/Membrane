#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { adaptMemoryBenchmark } from '../../benchmarks/memory/adapters.mjs';

const fail = reason => ({ status: 'open', reason });
export function verifyMemoryBenchmark(input) {
  try {
    const result = adaptMemoryBenchmark(input);
    if (result.componentUnderTest !== 'crypt-memory') return fail('componentUnderTest must be crypt-memory');
    if (input?.attribution || input?.marketingClaim || input?.membraneScore !== undefined || input?.managedScore !== undefined) return fail('Membrane or managed score attribution is forbidden');
    for (const group of ['retrieval', 'admission', 'product']) {
      if (!result.metrics[group] || Object.keys(result.metrics[group]).length === 0) return fail(`${group} metrics are required`);
    }
    return { schema: 'orthic.memory-benchmark-verification.v1', status: 'passed', benchmark: result.benchmark, componentUnderTest: result.componentUnderTest, role: result.role, identity: result.identity, metrics: result.metrics };
  } catch (error) { return fail(error.message); }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const path = process.argv[2];
  let input;
  try { input = JSON.parse(readFileSync(path, 'utf8')); } catch { process.stderr.write('input JSON is required\n'); process.exitCode = 2; }
  if (input) { const result = verifyMemoryBenchmark(input); process.stdout.write(`${JSON.stringify(result)}\n`); process.exitCode = result.status === 'passed' ? 0 : 2; }
}
