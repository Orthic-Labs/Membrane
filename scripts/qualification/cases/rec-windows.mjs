#!/usr/bin/env node
// REC-01..03 — canonical-records reconciliation for the Windows registry.
//
// These cases validate the registries/manifests that own reconciliation. They
// never rewrite canon or generated output: a stale generated artifact fails
// closed and remains the integration owner's repair.

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");
const REVIEW_ROOT_DEFAULT = "D:/Claude/review/windows-r5";
const PACKET_SHA256 = "7eed33c39ba37cbed704425a7c0845caa61c091bd9c70ed9c7ab3b0ebc423aee";
const REQUIRED_SUBCASES = Object.freeze({ BM: 12, NCL: 5, PKG: 5, LC: 6, EX: 9, REC: 3, CRA: 13 });

function rootOf(context = {}) {
  return resolve(context.workspaceRoot || REPO_ROOT);
}

function reviewRootOf(context = {}) {
  return resolve(context.reviewRoot || process.env.MEMBRANE_REVIEW_ROOT || REVIEW_ROOT_DEFAULT);
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}

function fail(message, detail = {}) {
  return { status: "failed", evidenceKind: "source", detail, reason: message };
}

function pass(detail, reason) {
  return { status: "passed", evidenceKind: "source", detail, reason };
}

function commandCheck(root, script, args = []) {
  const scriptPath = join(root, script.replaceAll("/", "\\"));
  const run = spawnSync(process.execPath, [scriptPath, ...args], {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    timeout: 120000,
    env: { ...process.env },
  });
  return {
    script,
    args,
    status: run.status,
    ok: run.error == null && run.status === 0,
    stdout: String(run.stdout || "").trim().slice(-2000),
    stderr: String(run.stderr || run.error?.message || "").trim().slice(-2000),
  };
}

export function currentNodeBlueprintRows(canonTexts) {
  const rows = [];
  for (const [file, text] of Object.entries(canonTexts)) {
    for (const [lineNumber, line] of String(text).replace(/\r\n/g, "\n").split("\n").entries()) {
      if (!line.trimStart().startsWith("|")) continue;
      if (!/\b(?:node\s+blueprint|blueprint\s+node)\b/i.test(line)) continue;
      if (/(?:legacy|retired|historical|delete-after-parity|void|removed)/i.test(line)) continue;
      rows.push({ file, line: lineNumber + 1, text: line.trim() });
    }
  }
  return rows;
}

export function evaluateCanonReconciliation({ canonTexts, generatedChecks, nodeRows = [] }) {
  const failures = [];
  if (!canonTexts || Object.keys(canonTexts).length !== 7) failures.push("canon inventory must contain seven subsystem canons");
  if (!Array.isArray(generatedChecks) || generatedChecks.some((check) => check.ok !== true)) failures.push("generated canon/product docs are stale or checker failed");
  if (nodeRows.length) failures.push(`canon row still names Node Blueprint as current: ${nodeRows.map((row) => `${row.file}:${row.line}`).join(", ")}`);
  return { ok: failures.length === 0, failures };
}

function canonTexts(root) {
  const dir = join(root, "docs", "canon");
  const files = existsSync(dir) ? readdirSync(dir).filter((file) => file.endsWith(".md") && file !== "README.md").sort() : [];
  return Object.fromEntries(files.map((file) => [file, readFileSync(join(dir, file), "utf8")]));
}

export async function REC_01({ workspaceRoot = REPO_ROOT } = {}) {
  const root = rootOf({ workspaceRoot });
  const generatedChecks = [
    commandCheck(root, "scripts/ci/check-atomic-canons.mjs"),
    commandCheck(root, "scripts/tools/productization/generate-product-truth.mjs", ["--check"]),
  ];
  const texts = canonTexts(root);
  const evaluation = evaluateCanonReconciliation({ canonTexts: texts, generatedChecks, nodeRows: currentNodeBlueprintRows(texts) });
  if (!evaluation.ok) return fail(`REC-01: ${evaluation.failures.join("; ")}`, { generatedChecks, failures: evaluation.failures });
  return pass({ canonFiles: Object.keys(texts), generatedChecks }, "REC-01: canon rows and generated docs reconcile through read-only source checkers");
}

function acceptancePath(context, name) {
  const review = reviewRootOf(context);
  const explicit = context?.[name];
  return explicit || join(review, name === "membraneAcceptancePath" ? "windows-acceptance.json" : name === "coderightAcceptancePath" ? "coderight-acceptance.json" : "windows-amendment-acceptance.json");
}

export function evaluateLegacyEvidence(registry) {
  const rows = Array.isArray(registry?.cases) ? registry.cases : [];
  const voidRows = rows.filter((row) => row.legacyEvidenceVoid === true);
  const rowsWithLegacyFiles = rows.filter((row) => Array.isArray(row.legacyEvidenceFiles) && row.legacyEvidenceFiles.length > 0);
  const failures = [];
  if (!Number.isInteger(registry?.legacyEvidenceVoidCount)) failures.push("legacyEvidenceVoidCount is missing or not an integer");
  else if (registry.legacyEvidenceVoidCount !== voidRows.length) failures.push(`legacyEvidenceVoidCount=${registry.legacyEvidenceVoidCount} but found ${voidRows.length} flagged rows`);
  const voidIds = new Set(voidRows.map((row) => row.id));
  const unvoidedLegacyRows = rowsWithLegacyFiles.filter((row) => !voidIds.has(row.id)).map((row) => row.id);
  const voidRowsWithoutFiles = voidRows.filter((row) => !rowsWithLegacyFiles.some((candidate) => candidate.id === row.id)).map((row) => row.id);
  if (unvoidedLegacyRows.length) failures.push(`legacy evidence files are not voided: ${unvoidedLegacyRows.join(", ")}`);
  if (voidRowsWithoutFiles.length) failures.push(`legacyEvidenceVoid rows lack delete-after-parity files: ${voidRowsWithoutFiles.join(", ")}`);
  for (const row of voidRows) {
    if (!Array.isArray(row.legacyEvidenceFiles) || row.legacyEvidenceFiles.length === 0) failures.push(`${row.id} has no legacyEvidenceFiles`);
    if (!String(row.nativeRequalification || "").trim()) failures.push(`${row.id} has no native requalification rule`);
    // Qualification state is the authoritative pending field in the row's Q table.
    const cells = String(row.canonicalQualificationRow || "").trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((cell) => cell.trim());
    if (cells[3] !== "PENDING") failures.push(`${row.id} legacy evidence is not held at qualification PENDING`);
  }
  return { ok: failures.length === 0, failures, voidCount: voidRows.length, voidIds: [...voidIds], legacyFileRows: rowsWithLegacyFiles.length };
}

export async function REC_02(context = {}) {
  const file = acceptancePath(context, "membraneAcceptancePath");
  if (!existsSync(file)) return fail(`REC-02: Membrane acceptance registry missing: ${file}`, { file });
  let registry;
  try { registry = readJson(file); } catch (error) { return fail(`REC-02: cannot read acceptance registry: ${error.message}`, { file }); }
  const evaluation = evaluateLegacyEvidence(registry);
  if (!evaluation.ok) return fail(`REC-02: ${evaluation.failures.join("; ")}`, { file, ...evaluation });
  return pass({ file, voidCount: evaluation.voidCount, voidIds: evaluation.voidIds }, "REC-02: legacy evidence rows remain explicitly void and qualification-pending until native requalification");
}

function caseIds(registry, field = "cases") {
  const rows = Array.isArray(registry?.[field]) ? registry[field] : [];
  return rows.map((row) => row.id).filter(Boolean);
}

export function evaluateCrosswalk({ membrane, coderight, blueprint, amendment, coderightAmendment, crosswalk, manifest, packetHash }) {
  const failures = [];
  if (caseIds(membrane).length !== 340) failures.push(`Membrane denominator changed: ${caseIds(membrane).length}, expected 340`);
  if (caseIds(coderight).length !== 213) failures.push(`CodeRight denominator changed: ${caseIds(coderight).length}, expected 213`);
  const allIds = [
    ...caseIds(blueprint),
    ...caseIds(amendment),
    ...caseIds(coderightAmendment, "amendmentCases"),
  ];
  const hasPrefix = (id, prefix) => String(id).startsWith(`${prefix}-`) || String(id).startsWith(prefix) && /^\d/.test(String(id).slice(prefix.length));
  const duplicateSubcases = allIds.filter((id, index) => allIds.indexOf(id) !== index);
  if (duplicateSubcases.length) failures.push(`duplicate amendment subcase IDs: ${[...new Set(duplicateSubcases)].join(", ")}`);
  for (const [prefix, expected] of Object.entries(REQUIRED_SUBCASES)) {
    const observed = allIds.filter((id) => hasPrefix(id, prefix)).length;
    if (observed !== expected) failures.push(`${prefix} subcase count changed: ${observed}, expected ${expected}`);
  }
  const rows = Array.isArray(crosswalk?.rows) ? crosswalk.rows : [];
  if (crosswalk?.items !== 128 || rows.length !== 128) failures.push(`requirement crosswalk must contain 128 items/rows (got ${crosswalk?.items}/${rows.length})`);
  const ids = new Set(rows.map((row) => row.id));
  for (const row of rows) {
    if (!/^(?:R|Z|M|S)\d+[A-Za-z0-9]*$/.test(String(row.id || ""))) failures.push(`invalid crosswalk ID: ${row.id}`);
    if (!Array.isArray(row.lanes) || row.lanes.length === 0) failures.push(`${row.id} has no owning lane`);
    if (!Array.isArray(row.caseIds) || row.caseIds.length === 0) failures.push(`${row.id} has no mapped case`);
  }
  if (manifest?.finalImplementationPacket?.sha256 !== PACKET_SHA256) failures.push("corrected-input-manifest does not pin supplied FINAL packet SHA-256");
  if (packetHash && packetHash !== PACKET_SHA256) failures.push(`supplied FINAL packet hash mismatch: ${packetHash}`);
  return { ok: failures.length === 0, failures, denominators: { membrane: caseIds(membrane).length, coderight: caseIds(coderight).length }, subcases: Object.fromEntries(Object.keys(REQUIRED_SUBCASES).map((prefix) => [prefix, allIds.filter((id) => hasPrefix(id, prefix)).length])), crosswalk: { items: crosswalk?.items, rows: rows.length, mapped: rows.filter((row) => Array.isArray(row.caseIds) && row.caseIds.length > 0).length }, packetSha256: manifest?.finalImplementationPacket?.sha256 };
}

function hashFile(file) {
  const hash = createHash("sha256");
  hash.update(readFileSync(file));
  return hash.digest("hex");
}

export async function REC_03(context = {}) {
  const review = reviewRootOf(context);
  const paths = {
    membrane: acceptancePath(context, "membraneAcceptancePath"),
    coderight: acceptancePath(context, "coderightAcceptancePath"),
    amendment: acceptancePath(context, "amendmentAcceptancePath"),
    blueprint: context.blueprintAcceptancePath || join(review, "blueprint-membrane-acceptance.json"),
    coderightAmendment: context.coderightAcceptancePath || join(review, "coderight-acceptance.json"),
    crosswalk: context.crosswalkPath || join(review, "requirement-crosswalk.json"),
    manifest: context.correctedInputManifestPath || join(review, "supplemental-inputs", "corrected-input-manifest.json"),
  };
  const missing = Object.entries(paths).filter(([, file]) => !existsSync(file)).map(([name, file]) => `${name}: ${file}`);
  if (missing.length) return fail(`REC-03: required reconciliation input missing: ${missing.join(", ")}`, { paths, missing });
  let input;
  try {
    input = Object.fromEntries(Object.entries(paths).map(([name, file]) => [name, readJson(file)]));
    const packetPath = input.manifest.finalImplementationPacket?.path;
    const packetHash = packetPath && existsSync(packetPath) ? hashFile(packetPath) : null;
    const evaluation = evaluateCrosswalk({ ...input, packetHash });
    if (!evaluation.ok) return fail(`REC-03: ${evaluation.failures.join("; ")}`, { paths, ...evaluation });
    return pass({ paths, ...evaluation }, "REC-03: denominators, amendment mappings, requirement crosswalk, and FINAL packet provenance reconcile");
  } catch (error) {
    return fail(`REC-03: reconciliation input is invalid: ${error.message}`, { paths });
  }
}

export const REC_CASES = Object.freeze({ REC_01, REC_02, REC_03 });
export const cases = REC_CASES;
