// Plan 3.1: RepositorySubgraphSliceV1 contract test.
//
// Validates the golden fixture against the schema's required fields, types,
// and enum values. No full JSON Schema validator is needed — the contract is
// the fixture file itself; this test proves it is structurally valid.
//
// The companion Python test (tests/test_repository_subgraph_slice_contract.py)
// validates the same fixture with the same assertions. One fixture, two
// validators — the "shared JS/Python" requirement of plan 3.1.

import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = JSON.parse(readFileSync(join(HERE, '../schemas/goldens/repository-subgraph-slice-v1.fixture.json'), 'utf8'));

test('golden fixture validates as RepositorySubgraphSliceV1', () => {
  // Top-level required fields
  assert.equal(FIXTURE.schemaVersion, 1);
  assert.equal(FIXTURE.kind, 'RepositorySubgraphSliceV1');
  assert.ok(FIXTURE.repoId === null || typeof FIXTURE.repoId === 'string');
  assert.ok(FIXTURE.generationId === null || typeof FIXTURE.generationId === 'string');
  assert.ok(FIXTURE.receiptId === null || typeof FIXTURE.receiptId === 'string');

  // Seeds: required path + reason, protected/exact are booleans
  assert.ok(Array.isArray(FIXTURE.seeds));
  assert.ok(FIXTURE.seeds.length > 0, 'golden fixture must have at least one seed');
  for (const seed of FIXTURE.seeds) {
    assert.equal(typeof seed.path, 'string');
    assert.equal(typeof seed.reason, 'string');
    if (seed.protected !== undefined) assert.equal(typeof seed.protected, 'boolean');
    if (seed.exact !== undefined) assert.equal(typeof seed.exact, 'boolean');
  }

  // Nodes: required id + kind + path, kind in enum
  assert.ok(Array.isArray(FIXTURE.nodes));
  const nodeKinds = new Set(['file', 'symbol', 'region', 'edge']);
  for (const node of FIXTURE.nodes) {
    assert.equal(typeof node.id, 'string');
    assert.equal(typeof node.path, 'string');
    assert.ok(nodeKinds.has(node.kind), `node kind must be one of ${[...nodeKinds]}, got: ${node.kind}`);
  }

  // Edges: required id + kind + source + confidenceTier
  assert.ok(Array.isArray(FIXTURE.edges));
  const tiers = new Set(['EXACT_RESOLUTION', 'STRONG_HEURISTIC', 'CROSS_FILE_HEURISTIC', 'UNRESOLVED']);
  for (const edge of FIXTURE.edges) {
    assert.equal(typeof edge.id, 'string');
    assert.equal(typeof edge.kind, 'string');
    assert.equal(typeof edge.source, 'string');
    assert.ok(tiers.has(edge.confidenceTier), `confidenceTier must be one of ${[...tiers]}, got: ${edge.confidenceTier}`);
  }

  // Bounds: required maxNodes, maxEdges, maxHops, budgetTokens (all integers)
  assert.equal(typeof FIXTURE.bounds.maxNodes, 'number');
  assert.equal(typeof FIXTURE.bounds.maxEdges, 'number');
  assert.equal(typeof FIXTURE.bounds.maxHops, 'number');
  assert.equal(typeof FIXTURE.bounds.budgetTokens, 'number');

  // Omissions: array, each has reason
  assert.ok(Array.isArray(FIXTURE.omissions));
  for (const om of FIXTURE.omissions) {
    assert.equal(typeof om.reason, 'string');
  }

  // continuationCursor and claims are optional; fixture may have them
  if (FIXTURE.continuationCursor !== null && FIXTURE.continuationCursor !== undefined) {
    assert.equal(typeof FIXTURE.continuationCursor, 'string');
  }
  if (FIXTURE.claims !== undefined) assert.ok(Array.isArray(FIXTURE.claims));
});

test('seeds carry exact-span provenance', () => {
  // Plan 3.1: each seed records why it was selected and carries symbol/exact metadata
  const seed = FIXTURE.seeds[0];
  assert.ok(seed.reason, 'first seed must have a reason');
  assert.ok(seed.exact !== undefined || seed.symbol !== undefined, 'seed should carry symbol or exact metadata');
});
