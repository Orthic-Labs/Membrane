#!/usr/bin/env node
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateReceipt as validatePlatformReceipt } from "../../scripts/release/verify-platform-artifacts.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const hash = value => typeof value === "string" && /^[0-9a-f]{40}$/.test(value);
const digest = value => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const read = path => JSON.parse(readFileSync(path, "utf8"));

export function acceptedReceipt(value, commit) {
  if (!value || value.schema !== "orthic.membrane.client-qualification.v1" || value.status !== "pass" || !hash(commit)) return false;
  if (typeof value.client !== "string" || !value.client || value.feature !== "installed-path" || !digest(value.evidenceDigest)) return false;
  if (!value.clientEvidence || !["client", "model", "host", "provider", "delivery", "outcome"].every(key => value.clientEvidence[key] === "pass")) return false;
  try { validatePlatformReceipt(value.platformReceipt); } catch { return false; }
  return value.platformReceipt.commit === commit;
}

function receipts(dir, commit) {
  if (!existsSync(dir)) return {};
  const found = {};
  for (const name of readdirSync(dir).sort()) {
    if (!name.endsWith(".json")) continue;
    try {
      const value = read(join(dir, name));
      if (acceptedReceipt(value, commit)) {
        const key = `${value.platformReceipt.platform}:${value.client}`;
        if (!found[key]) found[key] = value;
      }
    } catch { /* invalid evidence is unavailable */ }
  }
  return found;
}

export function buildMatrix({ commit, clients, receiptDirectory }) {
  const accepted = receipts(receiptDirectory, commit);
  const platforms = ["macos", "windows"];
  const rows = platforms.flatMap(platform => clients.map(client => ({
    platform, client,
    feature: "installed-path",
    tier: accepted[`${platform}:${client}`] ? "qualified" : "unavailable",
    sourceReady: false,
    receipt: accepted[`${platform}:${client}`] ? { receiptId: accepted[`${platform}:${client}`].receiptId, releaseGeneration: accepted[`${platform}:${client}`].platformReceipt.releaseGeneration, artifactSha256: accepted[`${platform}:${client}`].platformReceipt.artifact.sha256, evidenceDigest: accepted[`${platform}:${client}`].evidenceDigest } : null,
    reason: accepted[`${platform}:${client}`] ? "current client-specific clean-VM receipt" : "current client-specific clean-VM qualification receipt unavailable"
  })));
  return { schema: "orthic.membrane.support-tier-matrix.v1", generatedFrom: { commit, receiptDirectory: "evidence/qualification" }, rows };
}

export function render(matrix) {
  const lines = ["# Support-tier matrix", "", "Generated from current hash-bound clean-VM receipts. `unavailable` never means unsupported or healthy.", "", "| Platform | Client | Feature | Tier | Receipt |", "| --- | --- | --- | --- | --- |"];
  for (const row of matrix.rows) lines.push(`| ${row.platform} | ${row.client} | ${row.feature} | ${row.tier} | ${row.receipt?.receiptId ?? "unavailable"} |`);
  return `${lines.join("\n")}\n`;
}

export function generate({ repoRoot = root, commit = process.env.GIT_COMMIT } = {}) {
  const clients = read(join(repoRoot, "docs/clients/support-matrix.v1.json")).clients.map(({ id }) => id).sort();
  const currentCommit = commit ?? execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
  const matrix = buildMatrix({ commit: currentCommit, clients, receiptDirectory: join(repoRoot, "evidence/qualification") });
  writeFileSync(join(repoRoot, "docs/support-matrix.json"), `${JSON.stringify(matrix, null, 2)}\n`);
  writeFileSync(join(repoRoot, "docs/support-matrix.md"), render(matrix));
  return matrix;
}

if (import.meta.url === `file://${process.argv[1]}`) generate();
