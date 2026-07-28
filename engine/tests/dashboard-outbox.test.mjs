import assert from 'node:assert/strict';
import { webcrypto } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const DASHBOARD_PATH = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../crates/memright/src/dashboard.html',
);

function dashboardOutboxSource() {
  const html = readFileSync(DASHBOARD_PATH, 'utf8');
  const script = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
  assert.ok(script, 'dashboard must contain a script block');
  const outbox = script
    .replace('__MEMRIGHT_API_TOKEN_JSON__', 'null')
    .split('let current = null;')[0];
  assert.ok(outbox.includes('async function apiPut'), 'dashboard outbox source must be extractable');
  return `${outbox}\n;globalThis.__outbox = {
    MAX_PENDING_PUTS, MAX_PENDING_CLOCK_SKEW_MS, DEFAULT_PUT_RETRY_MS, MAX_PUT_RETRY_MS,
    pendingPuts, mutatePendingPuts, withPendingPutsLock, reservePendingPut, clearPendingPut,
    putIdentity, putDigest, retryablePutStatus, putRetryDelay, apiPut
  };`;
}

function createHarness({ now = 1_700_000_000_000, shared } = {}) {
  const sharedState = shared ?? { raw: null, lockTail: Promise.resolve(), uuid: 0 };
  const state = {
    now,
    get raw() { return sharedState.raw; },
    set raw(value) { sharedState.raw = value; },
    reads: 0,
    writes: 0,
    fetches: [],
    delays: [],
    fetchImpl: async () => {
      throw new Error('unexpected fetch');
    },
  };
  class MockDate extends Date {
    static now() { return state.now; }
  }
  const context = vm.createContext({
    console,
    Date: MockDate,
    JSON,
    Object,
    Number,
    Array,
    Promise,
    Error,
    String,
    RegExp,
    TextEncoder,
    crypto: {
      subtle: webcrypto.subtle,
      randomUUID: () => `00000000-0000-4000-8000-${String(++sharedState.uuid).padStart(12, '0')}`,
    },
    localStorage: {
      getItem: () => { state.reads += 1; return state.raw; },
      setItem: (_key, value) => { state.writes += 1; state.raw = String(value); },
    },
    location: { hash: '' },
    navigator: {
      locks: {
        request: (name, options, operation) => {
          assert.equal(name, 'memright.pendingPuts.v1.lock');
          assert.equal(options?.mode, 'exclusive');
          assert.deepEqual(Object.keys(options), ['mode']);
          const result = sharedState.lockTail.then(operation);
          sharedState.lockTail = result.catch(() => {});
          return result;
        },
      },
    },
    fetch: async (path, options) => {
      state.fetches.push({ path, options });
      return state.fetchImpl(path, options);
    },
    setTimeout: (operation, delay) => {
      state.delays.push(delay);
      queueMicrotask(operation);
      return state.delays.length;
    },
  });
  vm.runInContext(dashboardOutboxSource(), context, { filename: DASHBOARD_PATH });
  return { state, outbox: context.__outbox, context };
}

function response(status, body = {}, retryAfter = null) {
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: { get: name => name.toLowerCase() === 'retry-after' ? retryAfter : null },
    text: async () => JSON.stringify(body),
  };
}

function storedEntry(identity, { key = 'visible-key', createdAt } = {}) {
  return {
    key,
    createdAt,
    bodyFingerprint: identity.bodyFingerprint,
    authFingerprint: identity.authFingerprint,
  };
}

test('missing state initializes, while corrupt state fails closed without overwrite or fetch', async () => {
  const { state, outbox } = createHarness();
  state.fetchImpl = async () => response(400, { error: 'definitive' });
  const missing = await outbox.apiPut({ name: 'new', scope: 'global', content: 'safe' }, null, state.now);
  assert.equal(missing.error, 'definitive');
  assert.equal(state.fetches.length, 1);
  assert.equal(state.raw, '{}');

  const identity = await outbox.putIdentity('corrupt-entry', null);
  const corruptCases = [
    '{broken',
    '[]',
    'null',
    JSON.stringify({ bad: storedEntry(identity, { createdAt: state.now }) }),
    JSON.stringify({
      [identity.digest]: { ...storedEntry(identity, { createdAt: state.now }), body: 'must-not-persist' },
    }),
  ];
  for (const raw of corruptCases) {
    state.raw = raw;
    state.fetches.length = 0;
    await assert.rejects(
      outbox.apiPut({ name: 'blocked', scope: 'global', content: 'x' }, null, state.now),
      /operator action.*no request was sent/,
    );
    assert.equal(state.fetches.length, 0);
    assert.equal(state.raw, raw);
  }
});

test('entries have exactly four privacy-safe fields and auth rotation cannot duplicate an ambiguous body', async () => {
  const { state, outbox } = createHarness();
  const body = { name: 'same', scope: 'global', content: 'SECRET_BODY' };
  state.fetchImpl = async () => { throw new Error('network ambiguity'); };
  await assert.rejects(outbox.apiPut(body, 'token-A', state.now), /remains pending/);
  assert.equal(state.fetches.length, 2);
  const rawAfterA = state.raw;
  const [entry] = Object.values(JSON.parse(rawAfterA));
  assert.deepEqual(Object.keys(entry).sort(), [
    'authFingerprint', 'bodyFingerprint', 'createdAt', 'key',
  ]);
  assert.doesNotMatch(rawAfterA, /token-A|SECRET_BODY/);

  await assert.rejects(
    outbox.apiPut(body, 'token-B', state.now),
    /different auth principal.*no request was sent/,
  );
  assert.equal(state.fetches.length, 2);
  assert.equal(state.raw, rawAfterA);

  state.fetchImpl = async () => response(400, { error: 'different body allowed' });
  const result = await outbox.apiPut({ ...body, content: 'DIFFERENT_BODY' }, 'token-B', state.now);
  assert.equal(result.error, 'different body allowed');
  assert.equal(state.fetches.length, 3);
  assert.equal(Object.keys(outbox.pendingPuts(state.now)).length, 1);
  assert.doesNotMatch(state.raw, /token-B|DIFFERENT_BODY/);
});

test('same principal reuses ancient entries and enforces the five-minute future boundary', async () => {
  const { state, outbox } = createHarness();
  const identity = await outbox.putIdentity('same-body', 'token-A');
  const ancient = 1;
  state.raw = JSON.stringify({
    [identity.digest]: storedEntry(identity, { key: 'ancient-key', createdAt: ancient }),
  });
  assert.equal((await outbox.reservePendingPut(identity, state.now)).key, 'ancient-key');
  assert.equal(outbox.pendingPuts(state.now)[identity.digest].createdAt, ancient);

  state.raw = JSON.stringify({
    [identity.digest]: storedEntry(identity, {
      key: 'edge-key',
      createdAt: state.now + outbox.MAX_PENDING_CLOCK_SKEW_MS,
    }),
  });
  assert.equal((await outbox.reservePendingPut(identity, state.now)).key, 'edge-key');

  const tooFuture = JSON.stringify({
    [identity.digest]: storedEntry(identity, {
      key: 'future-key',
      createdAt: state.now + outbox.MAX_PENDING_CLOCK_SKEW_MS + 1,
    }),
  });
  state.raw = tooFuture;
  state.fetchImpl = async () => response(200, { put: 'must not happen' });
  await assert.rejects(
    outbox.apiPut(JSON.parse('"same-body"'), 'token-A', state.now),
    /corrupt.*no request was sent/,
  );
  assert.equal(state.fetches.length, 0);
  assert.equal(state.raw, tooFuture);
});

test('capacity counts every unresolved entry and rejects a new body before fetch', async () => {
  const { state, outbox } = createHarness();
  const entries = {};
  for (let index = 0; index < outbox.MAX_PENDING_PUTS; index += 1) {
    entries[index.toString(16).padStart(64, '0')] = {
      key: `key-${index}`,
      createdAt: 1,
      bodyFingerprint: (index + 100).toString(16).padStart(64, '0'),
      authFingerprint: (index + 200).toString(16).padStart(64, '0'),
    };
  }
  state.raw = JSON.stringify(entries);
  const before = state.raw;
  await assert.rejects(
    outbox.apiPut({ name: 'sixty-five' }, null, state.now),
    /64 saves.*no request was sent/,
  );
  assert.equal(state.fetches.length, 0);
  assert.equal(state.raw, before);
});

test('missing Web Locks support fails closed before fetch', async () => {
  const { state, outbox, context } = createHarness();
  context.navigator.locks = undefined;
  await assert.rejects(
    outbox.apiPut({ name: 'unsupported-browser' }, null, state.now),
    /cannot safely serialize.*no request was sent/,
  );
  assert.equal(state.fetches.length, 0);
  assert.equal(state.raw, null);
});

test('Web Locks serialize concurrent reservations and stale cleanup preserves B and newer A', async () => {
  const shared = { raw: null, lockTail: Promise.resolve(), uuid: 0 };
  const tabA = createHarness({ shared });
  const tabB = createHarness({ shared });
  const { state, outbox } = tabA;
  const [identityA, identityB, identityC] = await Promise.all([
    tabA.outbox.putIdentity('A'), tabB.outbox.putIdentity('B'), tabB.outbox.putIdentity('C'),
  ]);
  const [entryA] = await Promise.all([
    tabA.outbox.reservePendingPut(identityA, state.now),
    tabB.outbox.reservePendingPut(identityB, state.now),
  ]);
  assert.deepEqual(Object.keys(outbox.pendingPuts(state.now)).sort(), [
    identityA.digest, identityB.digest,
  ].sort());

  await Promise.all([
    tabA.outbox.clearPendingPut(identityA.digest, entryA.key, state.now),
    tabB.outbox.reservePendingPut(identityC, state.now),
  ]);
  assert.deepEqual(Object.keys(outbox.pendingPuts(state.now)).sort(), [
    identityB.digest, identityC.digest,
  ].sort());

  await tabB.outbox.withPendingPutsLock(() => tabB.outbox.mutatePendingPuts(entries => {
    entries[identityA.digest] = storedEntry(identityA, { key: 'newer-a', createdAt: state.now });
  }, state.now));
  await tabA.outbox.clearPendingPut(identityA.digest, entryA.key, state.now);
  assert.equal(outbox.pendingPuts(state.now)[identityA.digest].key, 'newer-a');
  assert.ok(outbox.pendingPuts(state.now)[identityB.digest]);
  assert.ok(outbox.pendingPuts(state.now)[identityC.digest]);
});

test('408, 429, every 5xx edge, and network ambiguity retry once with identical key and body', async t => {
  const cases = [
    { name: '408 default delay', status: 408, retryAfter: null, delay: 25 },
    { name: '429 capped seconds', status: 429, retryAfter: '999', delay: 2000 },
    { name: '500 date cap', status: 500, retryAfter: new Date(1_700_000_010_000).toUTCString(), delay: 2000 },
    { name: '599 remains retryable', status: 599, retryAfter: '0', delay: 0 },
    { name: 'network ambiguity', network: true, delay: 25 },
  ];
  for (const scenario of cases) {
    await t.test(scenario.name, async () => {
      const { state, outbox } = createHarness();
      let attempt = 0;
      state.fetchImpl = async () => {
        attempt += 1;
        if (attempt === 1 && scenario.network) throw new Error('ambiguous network');
        return attempt === 1
          ? response(scenario.status, { error: 'retry' }, scenario.retryAfter)
          : response(200, { put: 'ok' });
      };
      assert.equal((await outbox.apiPut({ name: scenario.name }, null, state.now)).put, 'ok');
      assert.equal(state.fetches.length, 2);
      assert.equal(
        state.fetches[0].options.headers['Idempotency-Key'],
        state.fetches[1].options.headers['Idempotency-Key'],
      );
      assert.equal(state.fetches[0].options.body, state.fetches[1].options.body);
      assert.deepEqual(state.delays, [scenario.delay]);
      assert.deepEqual(Object.keys(outbox.pendingPuts(state.now)), []);
    });
  }
});

test('terminal responses clean matching entries while exhausted ambiguity retains them', async t => {
  for (const status of [200, 400]) {
    await t.test(`HTTP ${status} is terminal`, async () => {
      const { state, outbox } = createHarness();
      state.fetchImpl = async () => response(status, status === 200 ? { put: 'ok' } : { error: 'bad' });
      await outbox.apiPut({ name: `terminal-${status}` }, null, state.now);
      assert.deepEqual(Object.keys(outbox.pendingPuts(state.now)), []);
    });
  }

  await t.test('nonretryable non-4xx response remains pending', async () => {
    const { state, outbox } = createHarness();
    state.fetchImpl = async () => response(302, {});
    const result = await outbox.apiPut({ name: 'redirect' }, null, state.now);
    assert.equal(result.pending, true);
    assert.equal(state.fetches.length, 1);
    assert.equal(Object.keys(outbox.pendingPuts(state.now)).length, 1);
  });

  for (const scenario of ['5xx', 'network']) {
    await t.test(`${scenario} exhaustion remains pending`, async () => {
      const { state, outbox } = createHarness();
      state.fetchImpl = scenario === 'network'
        ? async () => { throw new Error('ambiguous network'); }
        : async () => response(503, { error: 'busy' }, '0');
      await assert.rejects(outbox.apiPut({ name: scenario }, null, state.now), /remains pending/);
      assert.equal(state.fetches.length, 2);
      assert.equal(Object.keys(outbox.pendingPuts(state.now)).length, 1);
    });
  }
});
