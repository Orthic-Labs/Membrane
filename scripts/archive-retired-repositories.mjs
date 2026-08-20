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

const mode = value('--mode', 'export');
const output = resolve(value('--output', DEFAULT_OUTPUT));
const orthicLocal = resolve(value('--orthic-local', join(ROOT, '..', 'orthic')));
const requestedTag = value('--tag', null);
if (output !== DEFAULT_OUTPUT) {
  throw new Error(`archive output must be ${DEFAULT_OUTPUT}`);
}

function command(program, args, options = {}) {
  return execFileSync(program, args, {
    cwd: options.cwd ?? ROOT,
    encoding: options.binary ? null : 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    env: options.env ? { ...process.env, ...options.env } : process.env,
    input: options.input,
    stdio: [options.input === undefined ? 'ignore' : 'pipe', 'pipe', 'pipe'],
  });
}

function archiveError(code, message, details = {}) {
  const error = new Error(`${code}: ${message}`);
  error.code = code;
  error.details = details;
  return error;
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

function ghGraphql(query, fields = {}) {
  const args = ['api', 'graphql', '-f', `query=${query}`];
  for (const [name, value] of Object.entries(fields)) args.push('-F', `${name}=${value}`);
  return JSON.parse(command('gh', args));
}

function remoteRefs(repo) {
  const url = `git@github.com:${repo.owner}/${repo.name}.git`;
  const records = new Map();
  for (const line of command('git', ['ls-remote', '--heads', '--tags', url]).trim().split('\n')) {
    if (!line) continue;
    const [sha, rawRef] = line.split(/\s+/);
    const peeled = rawRef.endsWith('^{}');
    const ref = peeled ? rawRef.slice(0, -3) : rawRef;
    if (!/^[0-9a-f]{40}$/.test(sha)) {
      throw archiveError('ARCHIVE_INVALID_REF_SHA', `${repo.owner}/${repo.name} returned invalid ref SHA`, { sha, rawRef });
    }
    const existing = records.get(ref) ?? {
      ref,
      objectSha: null,
      commitSha: null,
      objectType: null,
      localArchiveRef: ref
        .replace('refs/heads/', `refs/archive/retired/${repo.slug}/heads/`)
        .replace('refs/tags/', `refs/archive/retired/${repo.slug}/tags/`),
    };
    if (peeled) existing.commitSha = sha;
    else {
      existing.objectSha = sha;
      existing.objectType = 'commit';
    }
    records.set(ref, existing);
  }
  for (const record of records.values()) {
    record.commitSha ??= record.objectSha;
    if (record.objectSha !== record.commitSha) record.objectType = 'tag';
  }
  return [...records.values()].sort((left, right) => left.ref.localeCompare(right.ref));
}

function paged(repo, endpoint) {
  return gh(`repos/${repo.owner}/${repo.name}/${endpoint}`, { paginate: true });
}

async function authenticatedProjectInventory() {
  const query = `query($login:String!) {
    organization(login:$login) { projectsV2(first:1) { totalCount } }
  }`;
  let payload;
  try {
    payload = ghGraphql(query, { login: 'Orthic-Labs' });
  } catch (error) {
    throw archiveError(
      'ARCHIVE_PROJECT_METADATA_INCOMPLETE',
      'authenticated Projects V2 inventory is unavailable; public HTML is not evidence',
      { cause: error.message },
    );
  }
  const total = payload?.data?.organization?.projectsV2?.totalCount;
  if (!Number.isInteger(total) || total < 0) {
    throw archiveError('ARCHIVE_PROJECT_METADATA_INCOMPLETE', 'Projects V2 totalCount was not returned', { payload });
  }
  if (total > 0) {
    throw archiveError(
      'ARCHIVE_PROJECT_METADATA_INCOMPLETE',
      'Projects V2 contains projects but current token has no verified item-level read:project inventory',
      { total, requiredScope: 'read:project' },
    );
  }
  return {
    source: 'https://api.github.com/graphql',
    method: 'authenticated organization.projectsV2.totalCount',
    completeness: 'complete',
    open: 0,
    closed: 0,
    total: 0,
    linkedItems: [],
    requiredScope: null,
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

function readManifest() {
  const path = join(output, 'manifest.json');
  if (!existsSync(path)) throw archiveError('ARCHIVE_MANIFEST_MISSING', `archive manifest is missing: ${path}`);
  return JSON.parse(readFileSync(path, 'utf8'));
}

function revParse(ref, cwd = ROOT, peel = false) {
  try {
    return command('git', ['rev-parse', '--verify', peel ? `${ref}^{}` : ref], { cwd }).trim();
  } catch {
    return null;
  }
}

function distinctCommitTips(manifest, sourceCommit = manifest.membraneHead) {
  const tips = new Set([sourceCommit, manifest.localOrthic?.head]);
  for (const repository of manifest.repositories ?? []) {
    for (const ref of repository.refs ?? []) tips.add(ref.commitSha ?? ref.objectSha);
  }
  tips.delete(null);
  tips.delete(undefined);
  return [...tips].sort();
}

function archiveRefRecords(manifest) {
  return (manifest.repositories ?? []).flatMap((repository) => (repository.refs ?? []).map((ref) => ({
    ...ref,
    repository: repository.repository,
    remoteUrl: repository.remoteUrl,
  })));
}

function ensureArchiveRefs(manifest) {
  for (const ref of archiveRefRecords(manifest)) {
    const current = revParse(ref.localArchiveRef);
    if (current) {
      if (current !== ref.objectSha) {
        throw archiveError('ARCHIVE_REF_CONFLICT', `${ref.localArchiveRef} already points at ${current}`, {
          expected: ref.objectSha,
        });
      }
      continue;
    }
    command('git', [
      'fetch', '--no-tags', ref.remoteUrl,
      `+${ref.ref}:${ref.localArchiveRef}`,
    ]);
    const fetched = revParse(ref.localArchiveRef);
    if (fetched !== ref.objectSha) {
      throw archiveError('ARCHIVE_REF_FETCH_MISMATCH', `${ref.localArchiveRef} fetched unexpected object`, {
        expected: ref.objectSha,
        actual: fetched,
      });
    }
  }
}

function verifyArchiveRefs(manifest, anchorCommit = manifest.anchor?.commit, cwd = ROOT) {
  if (!anchorCommit || !/^[0-9a-f]{40}$/.test(anchorCommit)) {
    throw archiveError('ARCHIVE_ANCHOR_MISSING', 'manifest has no valid archive anchor commit');
  }
  if (revParse(anchorCommit, cwd) !== anchorCommit || revParse(anchorCommit, cwd, true) !== anchorCommit) {
    throw archiveError('ARCHIVE_ANCHOR_UNREACHABLE', 'archive anchor commit is absent from local object storage', { anchorCommit });
  }
  const parentLine = command('git', ['show', '-s', '--format=%P', anchorCommit], { cwd }).trim();
  const actualParents = parentLine ? parentLine.split(/\s+/).sort() : [];
  const expectedParents = (manifest.anchor.parents ?? []).slice().sort();
  if (expectedParents.length && JSON.stringify(actualParents) !== JSON.stringify(expectedParents)) {
    throw archiveError('ARCHIVE_ANCHOR_PARENTS_MISMATCH', 'anchor parent set differs from manifest', {
      expected: expectedParents,
      actual: actualParents,
    });
  }
  for (const ref of archiveRefRecords(manifest)) {
    const objectSha = revParse(ref.localArchiveRef, cwd);
    if (objectSha !== ref.objectSha) {
      throw archiveError('ARCHIVE_REF_UNREACHABLE', `${ref.repository} ${ref.ref} is not preserved`, {
        ref: ref.localArchiveRef,
        expected: ref.objectSha,
        actual: objectSha,
      });
    }
    const type = command('git', ['cat-file', '-t', objectSha], { cwd }).trim();
    const expectedType = ref.objectType ?? (ref.objectSha === ref.commitSha ? 'commit' : 'tag');
    if (type !== expectedType) {
      throw archiveError('ARCHIVE_REF_TYPE_MISMATCH', `${ref.localArchiveRef} has type ${type}, expected ${expectedType}`);
    }
    if (revParse(ref.localArchiveRef, cwd, true) !== ref.commitSha) {
      throw archiveError('ARCHIVE_REF_TARGET_MISMATCH', `${ref.localArchiveRef} peeled target differs`, {
        expected: ref.commitSha,
      });
    }
    try {
      command('git', ['merge-base', '--is-ancestor', ref.commitSha, anchorCommit], { cwd });
    } catch {
      throw archiveError('ARCHIVE_REF_TIP_UNREACHABLE', `${ref.commitSha} is not an anchor ancestor`, { ref: ref.ref });
    }
  }
  for (const tip of distinctCommitTips(manifest, manifest.anchor?.sourceCommit)) {
    if (revParse(tip, cwd, true) !== tip) {
      throw archiveError('ARCHIVE_COMMIT_TIP_MISSING', `commit tip ${tip} is absent`, { tip });
    }
    try {
      command('git', ['merge-base', '--is-ancestor', tip, anchorCommit], { cwd });
    } catch {
      throw archiveError('ARCHIVE_COMMIT_TIP_UNREACHABLE', `${tip} is not an anchor ancestor`, { tip, anchorCommit });
    }
  }
  command('git', ['fsck', '--full', '--no-reflogs', '--connectivity-only'], { cwd });
  const tagName = manifest.anchor.annotatedTag;
  const tagObject = revParse(`refs/tags/${tagName}`, cwd);
  if (!tagObject || (cwd === ROOT && tagObject !== manifest.anchor.tagObject) || revParse(`refs/tags/${tagName}`, cwd, true) !== anchorCommit) {
    throw archiveError('ARCHIVE_TAG_UNREACHABLE', `archive tag ${tagName} is not attached to anchor`, { tagObject, anchorCommit });
  }
  return {
    anchorCommit,
    refs: archiveRefRecords(manifest).length,
    distinctCommitTips: distinctCommitTips(manifest).length,
    tag: tagName,
    tagObject,
    fsck: 'PASS',
  };
}

function createArchiveAnchor(manifest) {
  const sourceCommit = value('--source-commit', manifest.membraneHead);
  const currentHead = command('git', ['rev-parse', 'HEAD']).trim();
  if (currentHead !== sourceCommit) {
    throw archiveError('ARCHIVE_SOURCE_COMMIT_MISMATCH', 'HEAD must equal explicit archive source commit', {
      sourceCommit,
      currentHead,
    });
  }
  const tagName = requestedTag ?? 'archive/retired-repositories-final';
  if (!/^[A-Za-z0-9._/-]+$/.test(tagName) || tagName.startsWith('/') || tagName.endsWith('/') || tagName.includes('..')) {
    throw archiveError('ARCHIVE_TAG_INVALID', `invalid archive tag name: ${tagName}`);
  }
  ensureArchiveRefs(manifest);
  const parents = distinctCommitTips(manifest, sourceCommit);
  for (const tip of parents) {
    if (revParse(tip, ROOT, true) !== tip) throw archiveError('ARCHIVE_COMMIT_TIP_MISSING', `missing commit tip ${tip}`);
  }
  const tree = command('git', ['mktree'], { input: '' }).trim();
  const date = command('git', ['show', '-s', '--format=%cI', sourceCommit]).trim();
  const commit = command('git', [
    'commit-tree', tree,
    ...parents.flatMap((parent) => ['-p', parent]),
    '-m', 'Membrane retired repository archive anchor',
  ], {
    env: {
      GIT_AUTHOR_NAME: 'Membrane Archive',
      GIT_AUTHOR_EMAIL: 'archive@membrane.local',
      GIT_COMMITTER_NAME: 'Membrane Archive',
      GIT_COMMITTER_EMAIL: 'archive@membrane.local',
      GIT_AUTHOR_DATE: date,
      GIT_COMMITTER_DATE: date,
    },
  }).trim();
  const fullTagRef = `refs/tags/${tagName}`;
  const existingTag = revParse(fullTagRef);
  if (existingTag) {
    if (revParse(fullTagRef, ROOT, true) !== commit) {
      throw archiveError('ARCHIVE_TAG_EXISTS', `${tagName} already names another object`, { existingTag, commit });
    }
  } else {
    command('git', ['tag', '-a', tagName, commit, '-m', 'Membrane retired repository archive'], {
      env: {
        GIT_COMMITTER_NAME: 'Membrane Archive',
        GIT_COMMITTER_EMAIL: 'archive@membrane.local',
        GIT_COMMITTER_DATE: date,
      },
    });
  }
  const tagObject = revParse(fullTagRef);
  const anchor = {
    sourceCommit,
    commit,
    parents,
    annotatedTag: tagName,
    tagObject,
    archiveRefs: archiveRefRecords(manifest).map(({ repository, ref, objectSha, commitSha, objectType, localArchiveRef }) => ({
      repository, ref, objectSha, commitSha, objectType, localArchiveRef,
    })),
    finalRefreshRequiredAfterOrthicHardCut: true,
  };
  manifest.anchor = anchor;
  manifest.archiveProcedure = {
    mode: 'anchor',
    publish: {
      refs: archiveRefRecords(manifest).map((ref) => `${ref.localArchiveRef}:${ref.localArchiveRef}`),
      tag: fullTagRef,
    },
  };
  writeJson(join(output, 'manifest.json'), manifest);
  return verifyArchiveRefs(manifest);
}

function publishArchive(manifest) {
  const verification = verifyArchiveRefs(manifest);
  const refspecs = archiveRefRecords(manifest).map((ref) => `${ref.localArchiveRef}:${ref.localArchiveRef}`);
  command('git', ['push', 'origin', ...refspecs, `refs/tags/${manifest.anchor.annotatedTag}`]);
  return { ...verification, published: true, refspecs };
}

function freshCloneProof(manifest) {
  const cloneDir = resolve(value('--clone-dir', join(ROOT, '.archive-fresh-clone')));
  if (existsSync(cloneDir)) throw archiveError('ARCHIVE_CLONE_TARGET_EXISTS', `fresh-clone target exists: ${cloneDir}`);
  const remote = command('git', ['remote', 'get-url', 'origin']).trim();
  command('git', ['clone', remote, cloneDir]);
  const refspecs = archiveRefRecords(manifest).map((ref) => `+${ref.localArchiveRef}:${ref.localArchiveRef}`);
  command('git', ['fetch', 'origin', ...refspecs, `+refs/tags/${manifest.anchor.annotatedTag}:refs/tags/${manifest.anchor.annotatedTag}`], { cwd: cloneDir });
  const result = verifyArchiveRefs(manifest, manifest.anchor.commit, cloneDir);
  return { ...result, cloneDir, remote, freshCloneFsck: 'PASS' };
}

mkdirSync(output, { recursive: true });
if (mode === 'export') {
  const projectInventory = await authenticatedProjectInventory();
  const repositories = REPOSITORIES.map(exportRepository);
  const localOrthic = archiveLocalOrthic();
  const projectFile = writeJson(join(output, 'metadata', 'projects.json'), projectInventory);
  let existingAnchor = {
    sourceCommit: null,
    commit: null,
    annotatedTag: null,
    tagObject: null,
    finalRefreshRequiredAfterOrthicHardCut: true,
  };
  try {
    existingAnchor = JSON.parse(readFileSync(join(output, 'manifest.json'), 'utf8')).anchor ?? existingAnchor;
  } catch {}
  const manifest = {
    schema: 'membrane.retired-repository-archive.v1',
    generatedAt: new Date().toISOString(),
    membraneHead: command('git', ['rev-parse', 'HEAD']).trim(),
    repositories,
    projectInventory: { ...projectInventory, file: projectFile },
    localOrthic,
    anchor: existingAnchor,
  };
  writeJson(join(output, 'manifest.json'), manifest);
  process.stdout.write(`${JSON.stringify({
    output,
    mode,
    repositories: repositories.map((entry) => ({ repository: entry.repository, counts: entry.counts })),
    projects: projectInventory,
    localOrthic,
  })}\n`);
} else if (mode === 'anchor') {
  process.stdout.write(`${JSON.stringify(createArchiveAnchor(readManifest()))}\n`);
} else if (mode === 'verify') {
  process.stdout.write(`${JSON.stringify(verifyArchiveRefs(readManifest()))}\n`);
} else if (mode === 'publish') {
  process.stdout.write(`${JSON.stringify(publishArchive(readManifest()))}\n`);
} else if (mode === 'fresh-clone') {
  process.stdout.write(`${JSON.stringify(freshCloneProof(readManifest()))}\n`);
} else {
  throw archiveError('ARCHIVE_MODE_INVALID', `unsupported archive mode: ${mode}`);
}
