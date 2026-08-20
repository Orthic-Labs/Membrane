#!/usr/bin/env node
import { readFile } from "node:fs/promises";

const fail = (message) => { throw new Error(message); };
const digest = value => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const commit = value => typeof value === "string" && /^[0-9a-f]{40}$/.test(value);
const passed = value => value === "pass";
const safeId = value => typeof value === "string" && /^[A-Za-z0-9._:-]{1,128}$/.test(value);
const closed = (value, keys, label) => {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).some(key => !keys.includes(key))) fail(`${label} invalid`);
};

export function validateReceipt(receipt) {
  closed(receipt, ["schema", "receiptId", "mode", "commit", "releaseGeneration", "version", "platform", "artifact", "trust", "lifecycle", "environment"], "receipt");
  if (!receipt || receipt.schema !== "membrane.platform-acceptance.v1") fail("receipt schema invalid");
  if (!safeId(receipt.receiptId) || !commit(receipt.commit) || !digest(receipt.releaseGeneration) || typeof receipt.version !== "string" || !/^v?\d+\.\d+\.\d+$/.test(receipt.version)) fail("receipt identity invalid");
  closed(receipt.artifact, ["name", "sha256"], "artifact");
  if (!digest(receipt.artifact?.sha256) || typeof receipt.artifact?.name !== "string" || !receipt.artifact.name) fail("artifact identity invalid");
  if (!['macos', 'windows'].includes(receipt.platform)) fail("platform invalid");
  if (!['source-ready', 'clean-vm'].includes(receipt.mode)) fail("mode invalid");
  if (receipt.platform === "macos") {
    closed(receipt.trust, ["codesign", "notarization", "staple", "gatekeeper"], "macOS trust");
    const { codesign, notarization, staple, gatekeeper } = receipt.trust ?? {};
    if (![codesign, notarization, staple, gatekeeper].every(passed)) fail("macOS trust receipt incomplete");
  } else {
    closed(receipt.trust, ["authenticode", "publicTrust", "rfc3161"], "Windows trust");
    const { authenticode, publicTrust, rfc3161 } = receipt.trust ?? {};
    if (![authenticode, publicTrust, rfc3161].every(passed)) fail("Windows trust receipt incomplete");
  }
  closed(receipt.lifecycle, ["install", "startup", "update", "uninstall"], "lifecycle");
  for (const gate of ["install", "startup", "update", "uninstall"]) if (!passed(receipt.lifecycle?.[gate])) fail(`lifecycle receipt incomplete: ${gate}`);
  closed(receipt.environment, ["clean", "machineDigest", "bypassWarnings"], "environment");
  if (receipt.mode === "clean-vm" && (!receipt.environment.clean || !digest(receipt.environment.machineDigest) || receipt.environment.bypassWarnings !== false)) fail("clean-vm receipt needs isolated no-bypass evidence");
  return receipt;
}

export function verifyPair(contract, receipt) {
  if (contract?.schema !== "membrane.platform-acceptance.v1") fail("contract schema invalid");
  validateReceipt(receipt);
  for (const key of ["commit", "releaseGeneration", "version", "platform"]) if (contract[key] !== receipt[key]) fail(`contract mismatch: ${key}`);
  if (contract.artifact?.sha256 !== receipt.artifact.sha256 || contract.artifact?.name !== receipt.artifact.name) fail("contract mismatch: artifact");
  if (receipt.mode !== "clean-vm") fail("source-ready receipt is not clean-VM acceptance");
  return { status: "accepted", commit: receipt.commit, platform: receipt.platform, artifactSha256: receipt.artifact.sha256 };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [contractPath, receiptPath] = process.argv.slice(2);
  if (!contractPath || !receiptPath) fail("usage: verify-platform-artifacts.mjs CONTRACT.json RECEIPT.json");
  const [contract, receipt] = await Promise.all([contractPath, receiptPath].map(async path => JSON.parse(await readFile(path, "utf8"))));
  process.stdout.write(`${JSON.stringify(verifyPair(contract, receipt))}\n`);
}
