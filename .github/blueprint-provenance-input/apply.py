"""One-use, hash-bound source edit. Removed in the validated code commit."""
from pathlib import Path
import hashlib
import subprocess
import sys

ROOT = Path.cwd()
PAYLOAD = Path('.github/blueprint-provenance-input')
BEFORE = {
    'blueprint/release/compatibility.template.json': '824fc494007f6a2b0fbb732e35e862b55f0ec34a',
    'blueprint/src/graph/delta-store.mjs': 'e47f0e903c7dbc3d2eed09a036666e289150a582',
    'blueprint/src/graph/provenance.mjs': '77f03bc0d7f91ff3d34cbb25476658821fcd4e86',
    'blueprint/src/graph/static-provider.mjs': '6f6e8faa2eceb3d6d2f2304d53a405380e86ff18',
    'blueprint/src/graph/store-sqlite.mjs': 'da35f65a0cc95c90ac48e4b82f38aaae42d24bad',
}
ADDED = {
    'blueprint/src/graph/confidence-migration.mjs': 'confidence-migration.mjs',
    'blueprint/tests/confidence-migration.test.mjs': 'confidence-migration.test.mjs',
    'blueprint/tests/provenance-roundtrip.test.mjs': 'provenance-roundtrip.test.mjs',
}
MESSAGE = '''fix(blueprint-provenance): preserve null confidence through SQLite

Migrate symbol and edge confidence to nullable columns under the existing
transactional backup boundary. Preserve rowids, dense node order, indexes,
triggers and historical generation bodies instead of rewriting sealed facts.

Keep explicit nulls through full writes, file deltas, hydration and public
queries. Retain provenance in indexed symbol/edge responses; normalize tagged
legacy probabilities only in public views. Untagged V1 stays compatible.

Add real SCIP-to-SQLite, delta, v18-upgrade, rollback and query fixtures.
Update the nonpublishable compatibility template to store schema 19.
Validated by the focused Blueprint Node suite before this commit.
'''

def run(*args):
    return subprocess.check_output(args, text=True).strip()

def blob(path):
    data = path.read_bytes()
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()

def replace(path, old, new, count=1):
    p = ROOT / path
    s = p.read_text()
    if s.count(old) != count:
        raise RuntimeError(f'replacement precondition failed: {path}: {old[:60]}')
    p.write_text(s.replace(old, new))

def apply():
    if run('git', 'diff', '--name-only') or run('git', 'diff', '--cached', '--name-only'):
        raise RuntimeError('worktree/index is not clean')
    for path, expected in BEFORE.items():
        if blob(ROOT / path) != expected:
            raise RuntimeError(f'source advanced; refusing to overwrite {path}')
    for path in ADDED:
        if (ROOT / path).exists():
            raise RuntimeError(f'new-file collision: {path}')
    p = ROOT / 'blueprint/src/graph/provenance.mjs'
    p.write_text(p.read_text() + '''
// Older producers omitted confidence and received the V1 default. Explicit
// null is a real value in INV-004 and must survive storage/rehydration unchanged.
export function confidenceOrLegacyDefault(confidence) {
  return confidence === undefined ? 1 : confidence;
}

// Normalize a public view of a tagged legacy fact without rewriting its sealed
// generation. Untagged V1 data retains its original compatibility semantics.
export function publicFactConfidence(fact) {
  return fact?.provenance === undefined || fact?.provenance === null
    ? confidenceOrLegacyDefault(fact?.confidence)
    : confidenceForProvenance(fact.provenance, fact.confidence);
}
''')
    for path, source in ADDED.items():
        (ROOT / path).parent.mkdir(parents=True, exist_ok=True)
        (ROOT / path).write_bytes((ROOT / PAYLOAD / source).read_bytes())
    store = 'blueprint/src/graph/store-sqlite.mjs'
    replace(store, 'import { canonicalProviderId } from "./provider-identity.mjs";', 'import { canonicalProviderId } from "./provider-identity.mjs";\nimport { confidenceOrLegacyDefault, publicFactConfidence } from "./provenance.mjs";\nimport { migrateNullableFactConfidence } from "./confidence-migration.mjs";')
    replace(store, '];\n\n/** Current schema version = number of migrations.', '''  // Migration 19 — confidence is nullable for categorical facts. The existing
  // migration runner supplies the backup and atomic commit/rollback boundary.
  migrateNullableFactConfidence,
];

/** Current schema version = number of migrations.''')
    replace(store, 'stored.confidence ?? 1', 'confidenceOrLegacyDefault(stored.confidence)', 2)
    replace(store, 'edge.confidence ?? 1', 'confidenceOrLegacyDefault(edge.confidence)')
    p = ROOT / store
    s = p.read_text()
    a = s.index('function deserializeFileNodeRow(')
    b = s.index('/** Hydrate only requested nodes', a)
    part = s[a:b]
    if part.count('row.confidence ?? 1') != 4:
        raise RuntimeError('hydration boundary changed')
    s = s[:a] + part.replace('row.confidence ?? 1', 'confidenceOrLegacyDefault(row.confidence)') + s[b:]
    a = s.index('function deserializeSymbolRow(')
    b = s.index('// ---------------------------------------------------------------------------\n// Blast radius', a)
    s = s[:a] + '''function deserializeSymbolRow(row) {
  const fact = deserializeSymbolNodeRow(row);
  return {
    ...fact,
    confidence: publicFactConfidence(fact),
    generationId: row.generation_id,
  };
}

function deserializeEdgeRow(row) {
  const fact = deserializeEdgeNodeRow(row);
  return {
    ...fact,
    confidence: publicFactConfidence(fact),
    resolved: Boolean(row.resolved),
    specifier: row.specifier,
    generationId: row.generation_id,
  };
}

''' + s[b:]
    p.write_text(s)
    delta = 'blueprint/src/graph/delta-store.mjs'
    replace(delta, 'import { normalizeRepoPath } from "./path-order.mjs";', 'import { normalizeRepoPath } from "./path-order.mjs";\nimport { confidenceOrLegacyDefault } from "./provenance.mjs";')
    for expr in ['node.confidence', 'stored.confidence', 'edge.confidence']:
        replace(delta, expr + ' ?? 1', 'confidenceOrLegacyDefault(' + expr + ')')
    static = 'blueprint/src/graph/static-provider.mjs'
    replace(static, 'import { compareRepoPaths } from "./path-order.mjs";', 'import { compareRepoPaths } from "./path-order.mjs";\nimport { confidenceOrLegacyDefault, publicFactConfidence } from "./provenance.mjs";')
    p = ROOT / static
    s = p.read_text()
    if s.count('confidence: node.confidence ?? 1') != 3:
        raise RuntimeError('static confidence boundary changed')
    s = s.replace('confidence: node.confidence ?? 1', 'confidence: publicFactConfidence(node)', 2)
    s = s.replace('confidence: node.confidence ?? 1', 'confidence: confidenceOrLegacyDefault(node.confidence)')
    s = s.replace('confidence: publicFactConfidence(node),', 'confidence: publicFactConfidence(node),\n    ...(node.provenance === undefined ? {} : { provenance: node.provenance }),', 1)
    p.write_text(s)
    compat = 'blueprint/release/compatibility.template.json'
    replace(compat, '"store": 18', '"store": 19')
    replace(compat, '"currentSchemaVersion": 18', '"currentSchemaVersion": 19')
    run('git', 'diff', '--check')
    print(run('git', 'diff', '--stat'))

def commit():
    allowed = set(BEFORE) | set(ADDED)
    changed = set(run('git', 'diff', '--name-only').splitlines())
    if changed != set(BEFORE):
        raise RuntimeError(f'unexpected tracked changes: {changed}')
    run('git', 'add', '--', *sorted(allowed))
    staged = set(run('git', 'diff', '--cached', '--name-only').splitlines())
    if staged != allowed:
        raise RuntimeError(f'unexpected staged paths: {staged}')
    run('git', 'rm', '-r', '--', str(PAYLOAD))
    run('git', 'config', 'user.name', 'Blueprint completion automation')
    run('git', 'config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com')
    run('git', 'commit', '-m', MESSAGE)
    # Non-fast-forward pushes fail: never overwrite concurrent work or main.
    print(run('git', 'push', 'origin', 'HEAD:refs/heads/blueprint-completion'))

if __name__ == '__main__':
    if sys.argv[1:] == ['apply']:
        apply()
    elif sys.argv[1:] == ['commit']:
        commit()
    else:
        raise SystemExit('expected apply or commit')
