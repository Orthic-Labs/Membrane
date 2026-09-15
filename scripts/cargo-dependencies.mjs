import { spawnSync } from 'node:child_process';

const command = process.argv[2];
if (!['outdated', 'upgrade'].includes(command)) {
  console.error('usage: node scripts/cargo-dependencies.mjs <outdated|upgrade>');
  process.exit(2);
}

const manifests = [
  'engine/Cargo.toml',
  'apps/membrane-tray-windows/Cargo.toml',
  'apps/membrane-hub/src-tauri/Cargo.toml',
];

for (const manifest of manifests) {
  const result = spawnSync('rightkit', ['cargo', command, '--manifest-path', manifest], {
    cwd: new URL('..', import.meta.url),
    shell: process.platform === 'win32',
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
