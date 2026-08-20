#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  mkdirSync,
  existsSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { basename, join, relative, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const DEFAULT_OUTPUT = join(ROOT, 'docs', 'archive', 'retired-repositories');
const REPOSITORIES = [
  { owner: 'Orthic-Labs', name: 'Cortex', slug: 'cortex' },
  { owner: 'Orthic-Labs', name: 'Adapt', slug: 'adapt' },
  { owner: 'Orthic-Labs', name: 'orthic', slug: 'orthic' },
];

function value(flag, fallback = null) {
  const prefix = `${flag}=`;
  const argument = process.argv.slice(2).find((item) => item.startsWith(prefix));
  return argument ? argument.slice(prefix.length) : fallback;
}

const output = resolve(value('--output', DEFAULT_OUTPUT));
const orthicLocal = resolve(value('--orthic-local', join(ROOT, '..', 'orthic')));
if (output !== DEFAULT_OUTPUT) {
  throw new Error(`archive output must be ${DEFAULT_OUTPUT}`);
}

function command(program, args, options = {}) {
  return execFileSync(program, args, {
    cwd: options.cwd ?? ROOT,
    encoding: options.binary ? null : 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function writeJson(path, data) {
  mkdirSync(resolve(path, '..'), { recursive: true });
  const bytes = `${JSON.stringify(data, null, 2)}\n`;
  writeFileSync(path, bytes);
  return { path: relative(output, path), sha256: sha256(bytes), bytes: Buffer.byteLength(bytes) };
}

function gh(endpoint, { paginate = false, binary = false, headers = [] } = {}) {
  const args = ['api'];
  for (const header of headers) args.push('-H', header);
  if (paginate) args.push('--paginate', '--slurp');
  args.push(endpoint);
  const raw = command('gh', args, { binary });
  if (binary) return raw;
  const parsed = JSON.parse(raw);
  return paginate ? parsed.flat() : parsed;
}

function remoteRefs(repo) {
  const url = `git@github.com:${repo.owner}/${repo.name}.git`;
  const records = new Map();
  for (const line of command('git', ['ls-remote', '--heads', '--tags', url]).trim().split('\n')) {
    if (!line) continue;
    const [sha, rawRef] = line.split(/\s+/);
    const peeled = rawRef.endsWith('^{}');
    const ref = peeled ? rawRef.slice(0, -3) : rawRef;
    const existing = records.get(ref) ?? {
      ref,
      objectSha: null,
      commitSha: null,
      localArchiveRef: ref
        .replace('refs/heads/', `refs/archive/retired/${repo.slug}/heads/`)
        .replace('refs/tags/', `refs/archive/retired/${repo.slug}/tags/`),
    };
    if (peeled) existing.commitSha = sha;
    else existing.objectSha = sha;
    records.set(ref, existing);
  }
  for (const record of records.values()) record.commitSha ??= record.objectSha;
  return [...records.values()].sort((left, right) => left.ref.localeCompare(right.ref));
}

function paged(repo, endpoint) {
  return gh(`repos/${repo.owner}/${repo.name}/${endpoint}`, { paginate: true });
}

async function publicProjectInventory() {
  const url = 'https://github.com/orgs/Orthic-Labs/projects';
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok) throw new Error(`public project inventory failed: ${response.status}`);
  const html = await response.text();
  const match = html.match(/(\d+) open and (\d+) closed projects found\./);
  if (!match) throw new Error('public project inventory did not expose open/closed counts');
  return {
    source: url,
    method: 'public organization projects HTML',
    open: Number(match[1]),
    closed: Number(match[2]),
    total: Number(match[1]) + Number(match[2]),
    publicLinkedItems: Number(match[1]) + Number(match[2]) === 0 ? 0 : null,
    authenticatedInventory: 'unverified: current gh token lacks read:project; public organization inventory exposes zero projects',
  };
}

function archiveLocalOrthic() {
  const localDir = join(output, 'local-inputs');
  mkdirSync(localDir, { recursive: true });
  const staged = command('git', ['diff', '--cached', '--binary'], { cwd: orthicLocal });
  const unstaged = command('git', ['diff', '--binary'], { cwd: orthicLocal });
  const stagedPath = join(localDir, 'orthic-staged.patch');
  const currentStagedPath = join(localDir, 'orthic-current-staged.patch');
  const unstagedPath = join(localDir, 'orthic-unstaged.patch');
  if (staged.length > 0 || !existsSync(stagedPath)) writeFileSync(stagedPath, staged);
  writeFileSync(currentStagedPath, staged);
  writeFileSync(unstagedPath, unstaged);
  const preservedStaged = readFileSync(stagedPath);
  return {
    head: command('git', ['rev-parse', 'HEAD'], { cwd: orthicLocal }).trim(),
    originMain: command('git', ['rev-parse', 'origin/main'], { cwd: orthicLocal }).trim(),
    status: command('git', ['status', '--porcelain=v2', '--branch'], { cwd: orthicLocal }).trim().split('\n'),
    stagedPatch: { path: relative(output, stagedPath), sha256: sha256(preservedStaged), bytes: Buffer.byteLength(preservedStaged) },
    currentStagedPatch: { path: relative(output, currentStagedPath), sha256: sha256(staged), bytes: Buffer.byteLength(staged) },
    unstagedPatch: { path: relative(output, unstagedPath), sha256: sha256(unstaged), bytes: Buffer.byteLength(unstaged) },
  };
}

function exportRepository(repo) {
  const full = `${repo.owner}/${repo.name}`;
  const metadataDir = join(output, 'metadata', repo.slug);
  const assetDir = join(output, 'assets', repo.slug);
  rmSync(metadataDir, { recursive: true, force: true });
  rmSync(assetDir, { recursive: true, force: true });
  mkdirSync(metadataDir, { recursive: true });
  mkdirSync(assetDir, { recursive: true });

  const repository = gh(`repos/${full}`);
  const branches = paged(repo, 'branches?per_page=100');
  const tags = paged(repo, 'tags?per_page=100');
  const refs = remoteRefs(repo);
  const annotatedTags = refs
    .filter((ref) => ref.ref.startsWith('refs/tags/') && ref.objectSha !== ref.commitSha)
    .map((ref) => gh(`repos/${full}/git/tags/${ref.objectSha}`));
  const branchProtections = branches.map((branch) => branch.protected
    ? { branch: branch.name, protected: true, protection: gh(`repos/${full}/branches/${encodeURIComponent(branch.name)}/protection`) }
    : { branch: branch.name, protected: false, protection: null });
  const rulesets = paged(repo, 'rulesets?per_page=100');
  const issues = paged(repo, 'issues?state=all&per_page=100');
  const issueComments = paged(repo, 'issues/comments?per_page=100');
  const pulls = paged(repo, 'pulls?state=all&per_page=100');
  const pullReviewComments = paged(repo, 'pulls/comments?per_page=100');
  const reviews = pulls.flatMap((pull) => paged(repo, `pulls/${pull.number}/reviews?per_page=100`)
    .map((review) => ({ pullNumber: pull.number, ...review })));
  const releases = paged(repo, 'releases?per_page=100');
  const labels = paged(repo, 'labels?per_page=100');
  const milestones = paged(repo, 'milestones?state=all&per_page=100');
  const properties = gh(`repos/${full}/properties/values`);

  const files = [];
  for (const [name, data] of Object.entries({
    repository,
    branches,
    tags,
    'annotated-tags': annotatedTags,
    'branch-protections': branchProtections,
    rulesets,
    refs,
    issues,
    'issue-comments': issueComments,
    pulls,
    reviews,
    'pull-review-comments': pullReviewComments,
    releases,
    labels,
    milestones,
    properties,
  })) files.push(writeJson(join(metadataDir, `${name}.json`), data));

  const assets = [];
  for (const release of releases) {
    for (const asset of release.assets ?? []) {
      const bytes = gh(`repos/${full}/releases/assets/${asset.id}`, {
        binary: true,
        headers: ['Accept: application/octet-stream'],
      });
      const releaseDir = join(assetDir, release.tag_name.replaceAll('/', '__'));
      mkdirSync(releaseDir, { recursive: true });
      const assetPath = join(releaseDir, basename(asset.name));
      writeFileSync(assetPath, bytes);
      const digest = `sha256:${sha256(bytes)}`;
      if (asset.digest && asset.digest !== digest) {
        throw new Error(`${full} release asset digest mismatch for ${asset.name}`);
      }
      assets.push({
        id: asset.id,
        name: asset.name,
        source: asset.browser_download_url,
        path: relative(output, assetPath),
        bytes: bytes.length,
        sha256: digest.slice('sha256:'.length),
      });
    }
  }

  return {
    repository: full,
    remoteUrl: `git@github.com:${full}.git`,
    archived: repository.archived,
    refs,
    counts: {
      branches: branches.length,
      tags: tags.length,
      issuesAndPulls: issues.length,
      issuesOnly: issues.filter((issue) => !issue.pull_request).length,
      pulls: pulls.length,
      reviews: reviews.length,
      reviewComments: pullReviewComments.length,
      issueComments: issueComments.length,
      releases: releases.length,
      releaseAssets: assets.length,
      labels: labels.length,
      milestones: milestones.length,
      repositoryProperties: properties.length,
    },
    files,
    assets,
  };
}

mkdirSync(output, { recursive: true });
const projectInventory = await publicProjectInventory();
const repositories = REPOSITORIES.map(exportRepository);
const localOrthic = archiveLocalOrthic();
const projectFile = writeJson(join(output, 'metadata', 'projects.json'), projectInventory);
const manifest = {
  schema: 'membrane.retired-repository-archive.v1',
  generatedAt: new Date().toISOString(),
  membraneHead: command('git', ['rev-parse', 'HEAD']).trim(),
  repositories,
  projectInventory: { ...projectInventory, file: projectFile },
  localOrthic,
  anchor: {
    commit: null,
    annotatedTag: null,
    finalRefreshRequiredAfterOrthicHardCut: true,
  },
};
writeJson(join(output, 'manifest.json'), manifest);
process.stdout.write(`${JSON.stringify({
  output,
  repositories: repositories.map((entry) => ({ repository: entry.repository, counts: entry.counts })),
  projects: projectInventory,
  localOrthic,
})}\n`);
