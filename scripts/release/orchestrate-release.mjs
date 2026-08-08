#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const contract = JSON.parse(readFileSync(resolve(root, "release/contracts/pipeline.v1.json"), "utf8"));
const platforms = JSON.parse(readFileSync(resolve(root, "release/contracts/platforms.v1.json"), "utf8"));
const HEX40 = /^[0-9a-f]{40}$/; const HEX64 = /^[0-9a-f]{64}$/;
const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const required = (obj, key) => { if (typeof obj?.[key] !== "string" || !obj[key]) fail(`missing ${key}`); return obj[key]; };

export function validateRelease(input) {
  if (!input || typeof input !== "object") fail("manifest must be an object");
  const tag = required(input, "tag");
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) fail("tag must be immutable semver tag vX.Y.Z");
  const commit = required(input, "commit"); if (!HEX40.test(commit)) fail("commit must be 40-character lowercase git SHA");
  const tree = required(input, "tree"); if (!HEX40.test(tree) && !HEX64.test(tree)) fail("tree must be a lowercase git SHA");
  const generation = required(input, "release_generation"); if (!HEX40.test(generation) && !HEX64.test(generation)) fail("release_generation must be a lowercase hash");
  if (input.vector_dispatch !== contract.vector_dispatch) fail(`vector_dispatch must be ${contract.vector_dispatch}`);
  const target = required(input, "target"); if (!contract.targets.includes(target) || !platforms.platforms[target]) fail(`unsupported target: ${target}`);
  const artifact = required(input, "artifact_sha256"); if (!HEX64.test(artifact)) fail("artifact_sha256 must be SHA-256");
  const evidence = required(input, "evidence_sha256"); if (!HEX64.test(evidence)) fail("evidence_sha256 must be SHA-256");
  if (input.publish !== false) fail("publish must be false");
  if (!Array.isArray(input.stages) || JSON.stringify(input.stages) !== JSON.stringify(contract.stages)) fail("stages must be provenance,sbom,sign,platform_trust,verify in order");
  if (!input.provenance || !input.sbom || !input.sign || !input.platform_trust || !input.verify) fail("each stage requires immutable evidence reference");
  for (const stage of contract.stages) { const value = input[stage]; if (typeof value !== "object" || !HEX64.test(value.sha256 || "") || value.status !== "planned") fail(`${stage} evidence must be planned with SHA-256`); }
  const expectedTrust = platforms.platforms[target].os === "macos" ? "apple-notary" : "windows-authenticode";
  if (input.platform_trust.provider !== expectedTrust) fail(`${target} requires ${expectedTrust} platform trust`);
  return { ...input, schema: contract.schema, product: contract.product };
}

export function buildPlan(manifest) {
  const release = validateRelease(manifest);
  return { schema: contract.schema, product: contract.product, publish: false, identity: { tag: release.tag, commit: release.commit, tree: release.tree, release_generation: release.release_generation, target: release.target, support: platforms.platforms[release.target].support, artifact_sha256: release.artifact_sha256, evidence_sha256: release.evidence_sha256 }, vector_dispatch: contract.vector_dispatch, stages: contract.stages.map((stage) => ({ name: stage, status: "planned", evidence_sha256: release[stage].sha256 })), execution: "source-only; invoke platform builders/signers/verifiers separately" };
}

function cli() {
  const args = process.argv.slice(2); const path = args[args.indexOf("--manifest") + 1];
  if (!path || args.includes("--help")) { console.error("usage: orchestrate-release.mjs --manifest PATH [--validate]"); process.exit(path ? 0 : 2); }
  try { const input = JSON.parse(readFileSync(resolve(process.cwd(), path), "utf8")); const output = args.includes("--validate") ? validateRelease(input) : buildPlan(input); process.stdout.write(`${JSON.stringify(output, null, 2)}\n`); }
  catch (error) { console.error(error.message); process.exit(1); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
