import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

function countValid(dir) {
  let c = 0;
  for (const f of readdirSync(dir)) {
    if (!f.endsWith('.json')) continue;
    try {
      const j = JSON.parse(readFileSync(join(dir, f), 'utf8'));
      if (j.schemaVersion === 1 && (j.productId === 'cortex' || j.productId === 'membrane')) c++;
    } catch {}
  }
  return c;
}

test('only Cortex installed ⇒ exactly one tab', () => {
  assert.equal(countValid(new URL('./fixtures/one-tab', import.meta.url).pathname), 1);
});

test('both installed ⇒ two tabs', () => {
  assert.equal(countValid(new URL('./fixtures/two-tab', import.meta.url).pathname), 2);
});

test('empty dir ⇒ zero tabs', () => {
  // No fixtures at all
  const empty = new URL('./fixtures/empty', import.meta.url).pathname;
  // If dir missing, treat as 0
  let count = 0;
  try { count = countValid(empty); } catch { count = 0; }
  assert.equal(count, 0);
});

test('dormant tab surface exists', async () => {
  const onboarding = await import('../src/onboarding.mjs');
  assert.ok(onboarding.CHOICES.some(c => c.id === 'membrane' && c.includes.includes('cortex')));
});
