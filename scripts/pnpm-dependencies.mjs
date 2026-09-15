import { spawnSync } from 'node:child_process';

const command = process.argv[2];
if (!['outdated', 'upgrade'].includes(command)) {
  console.error('usage: node scripts/pnpm-dependencies.mjs <outdated|upgrade>');
  process.exit(2);
}

const roots = ['.', 'apps/membrane-hub'];
let exitCode = 0;
for (const root of roots) {
  const args = command === 'outdated' ? ['--dir', root, 'outdated'] : ['--dir', root, 'update', '--latest'];
  const result = spawnSync('pnpm', args, {
    cwd: new URL('..', import.meta.url),
    shell: process.platform === 'win32',
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) exitCode = result.status ?? 1;
}
process.exit(exitCode);
