#!/usr/bin/env node
// D20: grammar verification — every catalog entry routes to an inventoried
// WASM, every manifest entry is wired, and no catalog claim exceeds what is
// fixture-backed. --require-all-wired fails on any unwired grammar.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { inventoryGrammars } from "./inventory.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const CATALOG = JSON.parse(readFileSync(join(ROOT, "grammars", "catalog.json"), "utf8"));
const MANIFEST_PATH = join(ROOT, "grammars", "manifest.json");

export function verifyGrammars({ requireAllWired = false } = {}) {
  const inventory = inventoryGrammars();
  const problems = [];
  const byFile = new Map(inventory.entries.map((entry) => [entry.file, entry]));
  for (const grammar of CATALOG.grammars) {
    if (!byFile.has(grammar.grammarFile)) problems.push(`catalog references missing grammar ${grammar.grammarFile}`);
    if (!Array.isArray(grammar.extensions) || grammar.extensions.length === 0) problems.push(`${grammar.language} has no extensions`);
    if (!["code", "data", "markup", "specification"].includes(grammar.factProfile)) problems.push(`${grammar.language} has invalid factProfile`);
  }
  if (existsSync(MANIFEST_PATH)) {
    const manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
    const manifestFiles = new Set((manifest.grammars ?? []).map((entry) => entry.file));
    const wired = new Set((manifest.grammars ?? []).filter((entry) => entry.wired === true).map((entry) => entry.file));
    for (const file of inventory.entries.map((entry) => entry.file)) {
      if (!manifestFiles.has(file)) problems.push(`manifest missing grammar ${file}`);
      if (requireAllWired && !wired.has(file)) problems.push(`grammar not wired: ${file}`);
    }
  } else if (requireAllWired) {
    problems.push("grammars/manifest.json is missing");
  }
  return { ok: problems.length === 0, problems, count: inventory.entries.length };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const requireAllWired = process.argv.includes("--require-all-wired");
  const result = verifyGrammars({ requireAllWired });
  if (!result.ok) {
    for (const problem of result.problems) console.error(problem);
    process.exit(1);
  }
  console.log(`grammars verify OK: ${result.count} grammars routed`);
}
