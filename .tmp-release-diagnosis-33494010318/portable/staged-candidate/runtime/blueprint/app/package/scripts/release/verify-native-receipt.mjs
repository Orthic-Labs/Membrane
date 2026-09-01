#!/usr/bin/env node
// D17/D18: verify a native clean-host receipt before a release can proceed.
// Fails (non-zero) unless ALL hold: the receipt file exists and is readable;
// it is valid against schemas/native-clean-host-receipt-v1.schema.json
// (every field and stage required, nothing else allowed); its `version`
// matches --version; its `artifactSha256` is listed in the release catalog;
// and every stage is `true`. No stage ever defaults to true.

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";

const SCHEMA_PATH = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "schemas", "native-clean-host-receipt-v1.schema.json");
const STAGES = ["install", "init", "query", "mcp", "update", "rollback", "uninstall"];

const ajv = new Ajv2020({ allErrors: true, strict: true, allowUnionTypes: true, validateFormats: false });
const validate = ajv.compile(JSON.parse(readFileSync(SCHEMA_PATH, "utf8")));

function valueOf(argv, flag) {
  const index = argv.indexOf(flag);
  return index < 0 || argv[index + 1]?.startsWith("--") ? null : argv[index + 1];
}

export function verifyNativeReceipt({ receiptPath, catalogPath, version }) {
  const problems = [];
  let receipt;
  try {
    receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
  } catch (error) {
    problems.push(`receipt absent, unreadable, or not valid JSON: ${receiptPath} (${error.code ?? error.message})`);
    return { ok: false, problems };
  }
  if (!validate(receipt)) {
    problems.push(`receipt is not schema-valid (${validate.errors?.[0]?.instancePath ?? "/"} ${validate.errors?.[0]?.message ?? "invalid"})`);
    return { ok: false, problems };
  }
  if (receipt.version !== version) {
    problems.push(`receipt version ${receipt.version} does not match requested version ${version}`);
  }
  let catalog;
  try {
    catalog = JSON.parse(readFileSync(catalogPath, "utf8"));
  } catch (error) {
    problems.push(`catalog absent, unreadable, or not valid JSON: ${catalogPath} (${error.code ?? error.message})`);
    return { ok: false, problems };
  }
  if (!Array.isArray(catalog.artifacts) || !catalog.artifacts.length) problems.push(`catalog has no artifacts array: ${catalogPath}`);
  const catalogHashes = new Set(catalog.artifacts.map((artifact) => artifact.sha256).filter(Boolean));
  if (!catalogHashes.has(receipt.artifactSha256)) {
    problems.push(`artifactSha256 ${receipt.artifactSha256} is not present in ${catalogPath}`);
  }
  for (const stage of STAGES) {
    if (receipt.stages[stage] !== true) problems.push(`stage ${stage} is not true`);
  }
  return { ok: problems.length === 0, problems };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const argv = process.argv.slice(2);
  const receiptPath = valueOf(argv, "--receipt");
  const catalogPath = valueOf(argv, "--catalog");
  const version = valueOf(argv, "--version");
  if (!receiptPath || !catalogPath || !version) {
    console.error("usage: verify-native-receipt.mjs --receipt <path> --catalog <sealed-catalog.json> --version <v>");
    process.exit(2);
  }
  const result = verifyNativeReceipt({ receiptPath, catalogPath, version });
  if (!result.ok) {
    for (const problem of result.problems) console.error(`verify-native-receipt FAILED: ${problem}`);
    process.exit(1);
  }
  const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
  console.log(`verify-native-receipt OK: ${receipt.platform} ${receipt.version} (${receipt.runnerIdentity}) all stages passed`);
  process.exit(0);
}
