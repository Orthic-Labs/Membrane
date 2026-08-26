#!/usr/bin/env node

import { randomBytes } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { verifyPair } from "../release/verify-platform-artifacts.mjs";

const QUALIFICATION_SCHEMA = "membrane.windows-installed-qualification.v1";
const PLATFORM_SCHEMA = "membrane.platform-acceptance.v1";
const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;

const fail = message => { throw new Error(`write-windows-platform-evidence: ${message}`); };
const object = (value, label) => {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`);
  return value;
};
const string = (value, label) => {
  if (typeof value !== "string" || value.length === 0) fail(`${label} is required`);
  return value;
};
const digest = (value, label) => {
  if (typeof value !== "string" || !SHA256.test(value)) fail(`${label} must be lowercase SHA-256`);
  return value;
};

function regularFile(value, label) {
  const path = resolve(string(value, label));
  const stat = lstatSync(path);
  if (stat.isSymbolicLink() || !stat.isFile()) fail(`${label} must be a regular non-symlink file`);
  return path;
}

function readJson(value, label) {
  const path = regularFile(value, label);
  return { path, value: JSON.parse(readFileSync(path, "utf8")) };
}

function releaseIdentity(value) {
  const release = object(value.release ?? value, "release identity");
  const commit = string(release.commit, "release commit").toLowerCase();
  const releaseGeneration = string(release.generation ?? release.releaseGeneration, "release generation").toLowerCase();
  const version = string(release.version ?? release.tag, "release version");
  if (!COMMIT.test(commit)) fail("release commit must be lowercase 40-hex");
  digest(releaseGeneration, "release generation");
  if (!/^v?\d+\.\d+\.\d+$/.test(version)) fail("release version is invalid");
  return { commit, releaseGeneration, version };
}

export function buildEvidence(qualificationValue, releaseValue) {
  const qualification = object(qualificationValue, "qualification");
  if (qualification.schema !== QUALIFICATION_SCHEMA || qualification.platform !== "windows-x86_64" || qualification.profile !== "installed-local") {
    fail("qualification identity is invalid");
  }
  const artifactInput = object(qualification.artifact, "qualification artifact");
  const artifact = {
    name: basename(string(artifactInput.path, "qualification artifact.path")),
    sha256: digest(artifactInput.sha256, "qualification artifact.sha256"),
  };
  if (String(artifactInput.authenticode).toLowerCase() !== "valid") fail("qualification Authenticode is not valid");
  string(artifactInput.timestampSubject, "qualification timestampSubject");
  const signer = string(artifactInput.signerThumbprint, "qualification signerThumbprint").replace(/[^A-Za-z0-9._:-]/g, "");
  if (!signer) fail("qualification signerThumbprint is invalid");
  const lifecycleInput = object(qualification.lifecycle, "qualification lifecycle");
  for (const key of ["install", "startup", "upgrade", "uninstall"]) {
    if (String(lifecycleInput[key]).toLowerCase() !== "pass") fail(`qualification lifecycle.${key} is not pass`);
  }
  const identity = releaseIdentity(releaseValue);
  const common = { schema: PLATFORM_SCHEMA, ...identity, platform: "windows", artifact };
  const contract = { ...common };
  const receipt = {
    ...common,
    receiptId: `windows-installed-${artifact.sha256.slice(0, 16)}`,
    mode: "installed-local",
    trust: { authenticode: "pass", timestamp: "pass", publisher: `authenticode:${signer}` },
    lifecycle: { install: "pass", startup: "pass", update: "pass", uninstall: "pass" },
    environment: { host: "windows-laptop", bypassWarnings: false },
  };
  verifyPair(contract, receipt);
  return { contract, receipt };
}

function atomicJson(pathValue, value, label) {
  const path = resolve(string(pathValue, label));
  if (existsSync(path)) fail(`${label} already exists`);
  mkdirSync(dirname(path), { recursive: true });
  const staged = `${path}.${process.pid}.${randomBytes(8).toString("hex")}.staged`;
  try {
    writeFileSync(staged, `${JSON.stringify(value, null, 2)}\n`, { flag: "wx" });
    renameSync(staged, path);
  } catch (error) {
    rmSync(staged, { force: true });
    throw error;
  }
  return path;
}

function args(argv) {
  const out = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || !value) fail("arguments must be --name value pairs");
    out[key.slice(2)] = value;
  }
  return out;
}

export function main(argv = process.argv.slice(2)) {
  const input = args(argv);
  const qualification = readJson(input.qualification, "qualification").value;
  const release = readJson(input.release, "release identity").value;
  const evidence = buildEvidence(qualification, release);
  const contract = atomicJson(input["out-contract"], evidence.contract, "contract output");
  const receipt = atomicJson(input["out-receipt"], evidence.receipt, "receipt output");
  process.stdout.write(`${JSON.stringify({ contract, receipt, artifactSha256: evidence.receipt.artifact.sha256 })}\n`);
  return evidence;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
