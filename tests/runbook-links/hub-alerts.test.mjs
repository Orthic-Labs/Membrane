import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const map = fs.readFileSync(path.join(root, 'docs/troubleshooting/hub-alerts.md'), 'utf8');
const stableReasons = [
  'verified', 'evidence_unavailable', 'wrong_installation', 'incompatible_schema',
  'unexpected_data_root', 'delivered', 'not_selected', 'selected_without_delivery',
  'ready', 'degraded', 'unavailable', 'unknown', 'readiness_missing',
  'process_exists_without_readiness', 'identity_drift', 'observation_missing',
  'readiness_stale', 'test_query_failed', 'schema_version_mismatch',
  'required_identity_or_reason_missing', 'observation_freshness_invalid',
  'stream_not_configured', 'readiness_handle_missing', 'source_not_connected',
  'reason_unavailable', 'observed', 'authoritative',
];

function anchor(text) {
  return text.toLowerCase().replace(/[^a-z0-9 -]/g, '').replaceAll(' ', '-');
}

test('every stable Hub reason has a precise local runbook link and safe action', () => {
  for (const reason of stableReasons) {
    const line = map.split('\n').find((entry) => entry.includes(`\`${reason}\``));
    assert.ok(line, `missing map entry for ${reason}`);
    const match = line.match(/\]\(([^)]+)\)/);
    assert.ok(match, `missing runbook link for ${reason}`);
    const [file, fragment] = match[1].split('#');
    const target = fs.readFileSync(path.join(root, 'docs/troubleshooting', file), 'utf8');
    assert.ok(target, `missing target for ${reason}`);
    assert.ok(fragment && target.split('\n').some((heading) => heading.startsWith('#') && anchor(heading.replace(/^#+\s*/, '')) === fragment), `missing precise entry for ${reason}`);
    assert.match(line, /—\s+[^;.!?]+[.;!?]?/, `missing safe action for ${reason}`);
  }
});

test('operations index links canonical Hub and recovery runbooks', () => {
  const operations = fs.readFileSync(path.join(root, 'docs/operations.md'), 'utf8');
  for (const file of ['hub-alerts.md', 'diagnostics.md', 'backups.md', 'migrations.md', 'update-rollback.md', 'support-bundles.md']) {
    assert.match(operations, new RegExp(`troubleshooting/${file.replace('.', '\\.')}`));
  }
});
