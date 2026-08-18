import test from 'node:test';
import assert from 'node:assert/strict';
import { adaptLoCoMo, adaptLongMemEval, adaptBEAM } from './adapters.mjs';
import { verifyMemoryBenchmark } from '../../scripts/qualification/verify-memory-benchmark.mjs';

const base = { identity: { corpus: { id: 'synthetic', version: 'v1' }, model: 'fixture-model', hardware: 'cpu-fixture', release: 'release-fixture' }, metrics: { retrieval: { recall: 1 }, admission: { accepted: 1 }, product: { latencyMs: 1 } }, raw: { cases: [] } };
for (const [name, adapter] of [['LoCoMo', adaptLoCoMo], ['LongMemEval', adaptLongMemEval], ['BEAM', adaptBEAM]]) test(`${name} adapter preserves raw payload and role`, () => { const value = adapter({ ...base, benchmark: name, role: 'input' }); assert.equal(value.componentUnderTest, 'crypt-memory'); assert.equal(value.role, 'input'); assert.deepEqual(value.raw, base.raw); });
test('verifier separates metric groups', () => assert.equal(verifyMemoryBenchmark({ ...base, benchmark: 'LoCoMo' }).status, 'passed'));
test('verifier rejects missing identity and estimated values', () => { assert.equal(verifyMemoryBenchmark({ ...base, benchmark: 'LoCoMo', identity: { ...base.identity, hardware: '' } }).status, 'open'); assert.equal(verifyMemoryBenchmark({ ...base, benchmark: 'LoCoMo', metrics: { ...base.metrics, retrieval: { recall: { value: 1, estimated: true } } } }).status, 'open'); });
test('verifier rejects component and marketing attribution', () => { assert.equal(verifyMemoryBenchmark({ ...base, benchmark: 'LoCoMo', componentUnderTest: 'membrane' }).status, 'open'); assert.equal(verifyMemoryBenchmark({ ...base, benchmark: 'LoCoMo', membraneScore: 1 }).status, 'open'); });
