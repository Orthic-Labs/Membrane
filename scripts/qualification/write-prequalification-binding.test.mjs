import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { createPrequalificationBinding, writePrequalificationBinding } from './write-prequalification-binding.mjs';

const identity = {
  version: '0.1.12',
  commit: '0123456789abcdef0123456789abcdef01234567',
  tree: '89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567',
  generation: 'abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789',
  target: 'windows-x86_64',
};

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'membrane-prequalification-'));
  const installer = join(root, 'Membrane Hub_0.1.12_x64-setup.exe');
  const sbom = join(root, 'sbom.json');
  const out = join(root, 'RELEASE.provisional.json');
  await writeFile(installer, 'signed-installer-fixture');
  const hash = createHash('sha256').update(await readFile(installer)).digest('hex');
  await writeFile(sbom, JSON.stringify({
    schema: 'membrane.sbom.v1',
    artifact: { path: `.right-release/sealed/${basename(installer)}`, sha256: hash },
  }));
  return { root, installer, sbom, out, hash };
}

test('creates a hash-bound provisional release binding', async () => {
  const paths = await fixture();
  const binding = await writePrequalificationBinding({ ...identity, ...paths });
  assert.equal(binding.schema, 'membrane.release-evidence.v1');
  assert.equal(binding.provisional, true);
  assert.equal(binding.artifact.sha256, paths.hash);
  assert.equal(binding.release.artifact_sha256, paths.hash);
  assert.deepEqual(JSON.parse(await readFile(paths.out, 'utf8')), binding);
});

test('rejects installer tampering against existing SBOM', async () => {
  const paths = await fixture();
  await writeFile(paths.installer, 'tampered-installer-fixture');
  await assert.rejects(
    createPrequalificationBinding({ ...identity, ...paths }),
    /SBOM artifact\.sha256 does not match installer/,
  );
});

test('rejects an SBOM whose artifact path is not the installer', async () => {
  const paths = await fixture();
  await writeFile(paths.sbom, JSON.stringify({
    schema: 'membrane.sbom.v1',
    artifact: { path: 'other-installer.exe', sha256: paths.hash },
  }));
  await assert.rejects(
    createPrequalificationBinding({ ...identity, ...paths }),
    /SBOM artifact\.path does not identify installer/,
  );
});
