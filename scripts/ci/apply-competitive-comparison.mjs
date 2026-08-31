#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const comparisonRevision = "30b3c211ae874f369bed3fe92eb94b2fc5acbb16";
const files = ["membrane", "pull", "push", "cortex", "blueprint", "ledger", "adapt"];
const header = ["ID", "Parent", "Owner", "Scope", "Observable behavior", "Implementation", "Verification", "Qualification", "Delivery", "Action", "Evidence", "Competitive", "Comparison"];

function cells(line) { return line.trim().slice(1, -1).split("|").map((cell) => cell.trim()); }
function comparisonMap(file) {
  const relative = `docs/provenance/foundation/2026-08-31-competitive-comparison/${file}.md`;
  const absolute = path.join(root, relative), markdown = readFileSync(absolute, "utf8");
  const expected = ["Atom", "Scope", "Competitive disposition", "Best mechanism", "Current evidence", "Donor evidence", "Gap / action"];
  const lines = markdown.replace(/\r\n/g, "\n").split("\n"), start = lines.findIndex((line) => line.startsWith("|") && cells(line).join("|") === expected.join("|"));
  if (start < 0) throw new Error(`${file}: comparison table missing`);
  let end = start;
  while (end < lines.length && lines[end].startsWith("|")) end += 1;
  const rows = lines.slice(start, end);
  if (cells(rows[0]).join("|") !== expected.join("|")) throw new Error(`${file}: comparison schema mismatch`);
  const result = new Map();
  for (const line of rows.slice(2)) {
    const row = cells(line);
    if (row.length !== expected.length) throw new Error(`${file}: malformed comparison row`);
    if (result.has(row[0])) throw new Error(`${file}: duplicate comparison atom ${row[0]}`);
    result.set(row[0], { scope: row[1], disposition: row[2] });
  }
  const hash = execFileSync("git", ["hash-object", absolute], { cwd: root, encoding: "utf8" }).trim();
  return { relative, hash, rows: result };
}
function projectCanon(file, comparison) {
  const absolute = path.join(root, "docs", "canon", `${file}.md`), original = readFileSync(absolute, "utf8").replace(/\r\n/g, "\n");
  const lines = original.split("\n"), heading = lines.indexOf("## Capability ledger");
  if (heading < 0) throw new Error(`${file}: capability ledger missing`);
  let start = heading + 1;
  while (start < lines.length && !lines[start].startsWith("|")) start += 1;
  let end = start;
  while (end < lines.length && lines[end].startsWith("|")) end += 1;
  const sourceHeader = cells(lines[start]);
  if (sourceHeader.slice(0, 11).join("|") !== header.slice(0, 11).join("|")) throw new Error(`${file}: capability schema mismatch`);
  const projected = [`| ${header.join(" | ")} |`, `|${header.map(() => "---").join("|")}|`];
  const seen = new Set();
  for (const line of lines.slice(start + 2, end)) {
    const row = cells(line), id = row[0], receipt = comparison.rows.get(id);
    if (!receipt) throw new Error(`${file}: comparison omits ${id}`);
    if (receipt.scope !== row[3]) throw new Error(`${file}:${id}: scope differs from comparison`);
    const base = row.slice(0, 11);
    const evidence = `Receipt: ${comparison.relative}@${comparison.hash}; Atom: ${id}; Compared: ${comparisonRevision}`;
    projected.push(`| ${[...base, receipt.disposition, evidence].join(" | ")} |`);
    seen.add(id);
  }
  for (const id of comparison.rows.keys()) if (!seen.has(id)) throw new Error(`${file}: comparison has unknown atom ${id}`);
  lines.splice(start, end - start, ...projected);
  return { absolute, original, projected: lines.join("\n") };
}

let drift = false;
for (const file of files) {
  const result = projectCanon(file, comparisonMap(file));
  if (result.original === result.projected) continue;
  drift = true;
  if (process.argv.includes("--write")) writeFileSync(result.absolute, result.projected, "utf8");
}
if (drift && !process.argv.includes("--write")) throw new Error("competitive canon projection is stale; rerun with --write");
console.log(`competitive canon projection ${drift ? "updated" : "PASS"}: ${files.length} subsystem canons`);
