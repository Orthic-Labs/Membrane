// D11: redacted support bundles. Only allowlisted logical records are
// included; source content, absolute home paths, secrets, tokens, and raw
// environment values are excluded. Path values are rewritten as $HOME, $REPO,
// or repo IDs.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, relative, resolve, sep } from "node:path";
import { redactForEgress } from "../redaction.mjs";

export const SUPPORT_BUNDLE_ALLOWLIST = Object.freeze([
  "summary.json",
  "versions.json",
  "installation.json",
  "service-status.json",
  "repository-status.json",
  "doctor.json",
  "repair-plan.json",
  "logs/watchman-tail.log",
  "logs/service-tail.log",
  "checksums.txt",
]);

// D51: must match the extended corpus in fixtures/security/secret-corpus.json
// (npm tokens, fine-grained GitHub PATs, sk-live keys) and the redaction
// module's own SECRET_VALUE so a support bundle never contains a corpus entry
// that the lib/level redactor would have caught.
const SECRET_VALUE = /(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{30,}|npm_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|Bearer\s+[A-Za-z0-9._~-]+|sk-[A-Za-z0-9]{20,})/g;

function redactPath(value, root) {
  const str = String(value ?? "");
  return str
    .replaceAll(homedir(), "$HOME")
    .replaceAll(root, "$REPO")
    .replaceAll("\\", "/");
}

function redactRecordPaths(record, root) {
  if (Array.isArray(record)) return record.map((item) => redactRecordPaths(item, root));
  if (record && typeof record === "object") {
    return Object.fromEntries(Object.entries(record).map(([key, value]) =>
      [key, typeof value === "string" && /(root|path|home|dir|location)/i.test(key) ? redactPath(value, root) : redactRecordPaths(value, root)]));
  }
  return record;
}

export function checksumFile(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function buildSupportBundle({
  root = process.cwd(),
  outDir = ".agent",
  destination = null,
  records = {},
} = {}) {
  const bundleRoot = destination ?? resolve(root, outDir, "support-bundle");
  mkdirSync(bundleRoot, { recursive: true });
  mkdirSync(join(bundleRoot, "logs"), { recursive: true });

  const summary = {
    schemaVersion: 1,
    product: "cortex",
    createdAt: new Date().toISOString(),
    repoRoot: redactPath(root, root),
    records: SUPPORT_BUNDLE_ALLOWLIST,
  };
  writeFileSync(join(bundleRoot, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);

  const versions = {
    node: process.version,
    platform: `${process.platform}-${process.arch}`,
    packageChannel: records.packageChannel ?? "unknown",
  };
  writeFileSync(join(bundleRoot, "versions.json"), `${JSON.stringify(redactForEgress(versions), null, 2)}\n`);

  writeFileSync(join(bundleRoot, "installation.json"), `${JSON.stringify(redactForEgress(redactRecordPaths(records.installation ?? { scope: "unknown", root: redactPath(root, root) }, root)), null, 2)}\n`);
  writeFileSync(join(bundleRoot, "service-status.json"), `${JSON.stringify(redactForEgress(records.serviceStatus ?? { state: "unknown" }), null, 2)}\n`);
  writeFileSync(join(bundleRoot, "repository-status.json"), `${JSON.stringify(redactForEgress(records.repositoryStatus ?? { state: "unknown", repoIds: [] }), null, 2)}\n`);
  writeFileSync(join(bundleRoot, "doctor.json"), `${JSON.stringify(redactForEgress(records.doctor ?? { state: "unknown" }), null, 2)}\n`);
  writeFileSync(join(bundleRoot, "repair-plan.json"), `${JSON.stringify(redactForEgress(records.repairPlan ?? { schemaVersion: 1, actions: [] }), null, 2)}\n`);

  const logDir = join(bundleRoot, "logs");
  for (const [name, content] of [
    ["watchman-tail.log", records.watchmanLog ?? ""],
    ["service-tail.log", records.serviceLog ?? ""],
  ]) {
    const redacted = String(content ?? "")
      .split(/\r?\n/)
      .map((line) => redactPath(line, root).replace(SECRET_VALUE, "[REDACTED]"))
      .join("\n");
    writeFileSync(join(logDir, name), `${redacted}\n`);
  }

  const checksums = {};
  for (const name of SUPPORT_BUNDLE_ALLOWLIST) {
    const path = join(bundleRoot, name);
    if (existsSync(path)) checksums[name] = checksumFile(path);
  }
  writeFileSync(join(bundleRoot, "checksums.txt"), Object.entries(checksums).map(([name, hash]) => `${hash}  ${name}`).join("\n") + "\n");

  return { path: bundleRoot, files: SUPPORT_BUNDLE_ALLOWLIST, checksums };
}

export function archiveBundle(bundleRoot) {
  // Returns the path of the bundle directory; callers may tar it. Kept as a
  // no-op identity to keep this module dependency-free.
  return bundleRoot;
}
