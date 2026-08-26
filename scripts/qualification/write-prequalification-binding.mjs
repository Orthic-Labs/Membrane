#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { lstat, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { basename, dirname, isAbsolute, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCHEMA = 'membrane.release-evidence.v1';
const SBOM_SCHEMA = 'membrane.sbom.v1';
const TARGET = 'windows-x86_64';

function usage() {
  return 'Usage: node write-prequalification-binding.mjs --installer <path> --sbom <path> --version <version> --commit <40-hex> --tree <40|64-hex> --generation <64-hex> --target windows-x86_64 --out <path>';
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith('--')) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = argv[index + 1];
    if (!value || value.startsWith('--')) throw new Error(`Missing value for --${key}`);
    args[key] = value;
    index += 1;
  }
  return args;
}

async function requireRegularFile(filePath, label) {
  let info;
  try {
    info = await lstat(filePath);
  } catch (error) {
    throw new Error(`${label} is not readable: ${filePath}`, { cause: error });
  }
  if (info.isSymbolicLink() || !info.isFile()) {
    throw new Error(`${label} must be a regular non-symlink file: ${filePath}`);
  }
}

async function sha256(filePath) {
  const digest = createHash('sha256');
  digest.update(await readFile(filePath));
  return digest.digest('hex');
}

function canonicalPath(value) {
  return normalize(value).replaceAll('\\', '/').toLowerCase();
}

function artifactPathMatches(artifactPath, installerPath, sbomPath) {
  if (typeof artifactPath !== 'string' || artifactPath.length === 0) return false;
  const installer = resolve(installerPath);
  const declared = canonicalPath(artifactPath);
  if (isAbsolute(artifactPath)) return resolve(artifactPath).toLowerCase() === installer.toLowerCase();
  const relativeToSbom = resolve(dirname(sbomPath), artifactPath);
  return relativeToSbom.toLowerCase() === installer.toLowerCase()
    || canonicalPath(basename(artifactPath)) === canonicalPath(basename(installer));
}

function requireIdentity(args) {
  const required = ['version', 'commit', 'tree', 'generation', 'target'];
  for (const field of required) {
    if (typeof args[field] !== 'string' || args[field].length === 0) {
      throw new Error(`Missing --${field}`);
    }
  }
  if (args.target !== TARGET) throw new Error(`--target must be ${TARGET}`);
  if (!/^[0-9a-f]{40}$/i.test(args.commit)) throw new Error('--commit must be 40 hexadecimal characters');
  if (!/^[0-9a-f]{40}(?:[0-9a-f]{24})?$/i.test(args.tree)) {
    throw new Error('--tree must be 40 or 64 hexadecimal characters');
  }
  if (!/^[0-9a-f]{64}$/i.test(args.generation)) {
    throw new Error('--generation must be 64 hexadecimal characters');
  }
}

export async function createPrequalificationBinding({
  installer,
  sbom,
  version,
  commit,
  tree,
  generation,
  target,
}) {
  const args = { installer, sbom, version, commit, tree, generation, target };
  await requireRegularFile(installer, 'Installer');
  await requireRegularFile(sbom, 'SBOM');
  requireIdentity(args);

  let document;
  try {
    document = JSON.parse(await readFile(sbom, 'utf8'));
  } catch (error) {
    throw new Error(`SBOM is not valid JSON: ${sbom}`, { cause: error });
  }
  if (document?.schema !== SBOM_SCHEMA) throw new Error(`SBOM schema must be ${SBOM_SCHEMA}`);

  const installerSha256 = await sha256(installer);
  if (document?.artifact?.sha256 !== installerSha256) {
    throw new Error('SBOM artifact.sha256 does not match installer');
  }
  if (!artifactPathMatches(document?.artifact?.path, installer, sbom)) {
    throw new Error('SBOM artifact.path does not identify installer');
  }

  const releaseTag = version.startsWith('v') ? version : `v${version}`;
  return {
    schema: SCHEMA,
    provisional: true,
    artifact: {
      path: installer,
      sha256: installerSha256,
    },
    release: {
      tag: releaseTag,
      version: version.replace(/^v/, ''),
      commit: commit.toLowerCase(),
      tree: tree.toLowerCase(),
      generation: generation.toLowerCase(),
      target,
      artifact_sha256: installerSha256,
      provisional: true,
    },
    sbom: {
      path: sbom,
      schema: SBOM_SCHEMA,
      artifact_sha256: document.artifact.sha256,
    },
  };
}

export async function writePrequalificationBinding({ out, ...input }) {
  if (typeof out !== 'string' || out.length === 0) throw new Error('Missing --out');
  if (await lstat(out).then(() => true, () => false)) {
    throw new Error(`Refusing to overwrite existing output: ${out}`);
  }
  const binding = await createPrequalificationBinding(input);
  await mkdir(dirname(resolve(out)), { recursive: true });
  const temporary = `${resolve(out)}.${process.pid}.${Date.now()}.tmp`;
  try {
    await writeFile(temporary, `${JSON.stringify(binding, null, 2)}\n`, { flag: 'wx' });
    await rename(temporary, resolve(out));
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
  return binding;
}

export async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const binding = await writePrequalificationBinding({
    installer: args.installer,
    sbom: args.sbom,
    version: args.version,
    commit: args.commit,
    tree: args.tree,
    generation: args.generation,
    target: args.target,
    out: args.out,
  });
  process.stdout.write(`${JSON.stringify({ ok: true, provisional: binding.provisional, artifact_sha256: binding.artifact.sha256, out: resolve(args.out) })}\n`);
  return binding;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n${usage()}\n`);
    process.exitCode = 1;
  });
}
