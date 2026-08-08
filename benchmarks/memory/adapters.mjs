import { COMPONENT, makeResult } from './contracts.mjs';

const copy = value => value && typeof value === 'object' ? structuredClone(value) : value;
const identityOf = input => input.identity ?? input.metadata ?? {};
const metricsOf = input => input.metrics ?? { retrieval: input.retrieval, admission: input.admission, product: input.product };

function adapt(benchmark, input, role) {
  if (!input || typeof input !== 'object') throw new Error(`${benchmark} payload is required`);
  return makeResult({ benchmark, identity: identityOf(input), metrics: metricsOf(input), raw: copy(input.raw ?? input), role: role ?? input.role ?? 'result', componentUnderTest: input.componentUnderTest ?? COMPONENT });
}

export const adaptLoCoMo = (input, role) => adapt('LoCoMo', input, role);
export const adaptLongMemEval = (input, role) => adapt('LongMemEval', input, role);
export const adaptBEAM = (input, role) => adapt('BEAM', input, role);

export function adaptMemoryBenchmark(input) {
  const name = input?.benchmark ?? input?.name;
  if (name === 'LoCoMo') return adaptLoCoMo(input, input.role ?? 'result');
  if (name === 'LongMemEval') return adaptLongMemEval(input, input.role ?? 'result');
  if (name === 'BEAM') return adaptBEAM(input, input.role ?? 'result');
  throw new Error('benchmark must be LoCoMo, LongMemEval, or BEAM');
}
