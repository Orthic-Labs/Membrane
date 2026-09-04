"""One-use reviewed patch transport; deletes itself after scoped test-gated commits."""
from pathlib import Path
import hashlib
import json
import os
import subprocess

ROOT = Path.cwd()
INPUT = ROOT / '.github/blueprint-completion-input'
BRANCH = 'blueprint-completion'


def run(*args, cwd=ROOT, capture=False):
    result = subprocess.run(args, cwd=cwd, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout.strip() if capture else None


def blob(path):
    if not path.exists():
        return None
    if path.is_symlink() or not path.is_file():
        raise RuntimeError(f'non-regular source: {path}')
    data = path.read_bytes()
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()


def validate(hashes):
    for name, expected in hashes.items():
        path = Path(name)
        if not name.startswith('blueprint/') or path.is_absolute() or '..' in path.parts:
            raise RuntimeError(f'out-of-scope source: {name}')
        if blob(ROOT / path) != expected:
            raise RuntimeError(f'source-hash precondition failed: {name}')


def main():
    if os.environ.get('GITHUB_REF') != f'refs/heads/{BRANCH}':
        raise RuntimeError('patch transport is restricted to the completion branch')
    if run('git', 'status', '--porcelain', '--untracked-files=no', capture=True):
        raise RuntimeError('tracked checkout must be clean')
    run('git', 'config', 'user.name', 'Blueprint completion automation')
    run('git', 'config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com')
    series = json.loads((INPUT / 'series.json').read_text())
    for change in series:
        if set(change['before']) != set(change['after']):
            raise RuntimeError('source path sets differ')
        patch = INPUT / change['patch']
        if patch.parent != INPUT or patch.suffix != '.patch':
            raise RuntimeError('unexpected patch path')
        if hashlib.sha256(patch.read_bytes()).hexdigest() != change['sha256']:
            raise RuntimeError('patch transport checksum mismatch')
        validate(change['before'])
        run('git', 'apply', '--check', str(patch))
        run('git', 'apply', str(patch))
        validate(change['after'])
        run('git', 'diff', '--check')
        tests = change['tests']
        if not tests or any(not t.startswith('tests/') or not t.endswith('.test.mjs') or '..' in Path(t).parts for t in tests):
            raise RuntimeError('only declared Node test files are permitted')
        run('node', '--test', *tests, cwd=ROOT / 'blueprint')
        run('git', 'add', '--', *sorted(change['after']))
        staged = set(run('git', 'diff', '--cached', '--name-only', capture=True).splitlines())
        if staged != set(change['after']):
            raise RuntimeError(f'unexpected staged paths: {staged}')
        run('git', 'commit', '-m', change['message'])
    run('git', 'rm', '-r', '--', '.github/blueprint-completion-input')
    run('git', 'commit', '-m', 'ci(blueprint): remove consumed test-gated patch transport\n\nEvery declared scoped source change passed its Node regressions. Remove the one-use payloads; the broader no-build corpus remains a separate acceptance gate.')
    # A concurrent push fails here; never use --force or replace main.
    run('git', 'push', 'origin', f'HEAD:refs/heads/{BRANCH}')


if __name__ == '__main__':
    main()
