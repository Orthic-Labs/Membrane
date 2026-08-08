export const COMPONENT = 'crypt-memory';
export const BENCHMARKS = ['LoCoMo', 'LongMemEval', 'BEAM'];
export const ROLES = ['input', 'result', 'baseline', 'holdout'];

const nonEmpty = (value, name) => {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${name} is required`);
  return value.trim();
};

export function assertIdentity(identity = {}) {
  return {
    corpus: { id: nonEmpty(identity.corpus?.id, 'corpus.id'), version: nonEmpty(identity.corpus?.version, 'corpus.version') },
    model: nonEmpty(identity.model, 'model'),
    hardware: nonEmpty(identity.hardware, 'hardware'),
    release: nonEmpty(identity.release, 'release')
  };
}

export function assertMetrics(metrics = {}) {
  const groups = ['retrieval', 'admission', 'product'];
  const out = {};
  for (const group of groups) {
    if (!metrics[group] || typeof metrics[group] !== 'object') throw new Error(`${group} metrics are required`);
    out[group] = { ...metrics[group] };
    for (const [key, value] of Object.entries(out[group])) {
      if (value && typeof value === 'object' && value.estimated === true) throw new Error(`${group}.${key} is estimated, not measured`);
      if (typeof value === 'number' && !Number.isFinite(value)) throw new Error(`${group}.${key} is not finite`);
    }
  }
  return out;
}

export function makeResult({ benchmark, identity, metrics, raw, role = 'result', componentUnderTest = COMPONENT }) {
  if (!BENCHMARKS.includes(benchmark)) throw new Error(`unsupported benchmark: ${benchmark}`);
  if (componentUnderTest !== COMPONENT) throw new Error(`componentUnderTest must be ${COMPONENT}`);
  if (!ROLES.includes(role)) throw new Error(`role must be ${ROLES.join(', ')}`);
  if (!raw || typeof raw !== 'object') throw new Error('raw benchmark payload is required');
  return { schema: 'orthic.memory-benchmark.v1', benchmark, componentUnderTest, role, identity: assertIdentity(identity), metrics: assertMetrics(metrics), raw };
}
