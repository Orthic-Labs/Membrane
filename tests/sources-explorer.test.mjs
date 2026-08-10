import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
test('membrane-specific renderer deleted per D-2', () => {
  // These 7 renderers migrated to membrane repo; hub no longer owns them.
  // Verify they are deleted.
  const deleted = ["actions.mjs","agents-adapters.mjs","delivery-trace.mjs","fleet.mjs","memory-sentinel.mjs","release-channel.mjs","sources.mjs"];
  for (const f of deleted) {
    assert.equal(existsSync(new URL(`../src/${f}`, import.meta.url).pathname), false, `${f} should be deleted`);
  }
});
