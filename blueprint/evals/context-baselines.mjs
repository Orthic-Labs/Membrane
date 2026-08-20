#!/usr/bin/env node
import { cpSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import { recordTokenSavings } from "../src/lib/telemetry.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";

const ROOT = resolve(import.meta.dirname, "..");
const FIXTURES = join(ROOT, "evals", "fixture-repos");
const TASKS = [
  ["typescript-commerce", "order service", "src/service.ts"],
  ["typescript-commerce", "order store", "src/store.ts"],
  ["typescript-commerce", "routes", "src/routes.ts"],
  ["mixed-doc", "document truth", "README.md"],
  ["mixed-doc", "architecture", "docs/ARCHITECTURE.md"],
  ["python-context", "python entrypoint", "src/main.py"],
  ["python-context", "python imports", "src/main.py"],
  ["rust-audio", "rust audio", "src/lib.rs"],
  ["rust-audio", "rust module", "src/lib.rs"],
  ["typescript-commerce", "place order flow", "src/routes.ts"],
];

function filesUnder(root) {
  const result = [];
  const visit = (directory) => {
    for (const name of readdirSync(directory)) {
      if ([".git", ".agent", "node_modules"].includes(name)) continue;
      const absolute = join(directory, name);
      const stat = statSync(absolute);
      if (stat.isDirectory()) visit(absolute);
      else if (stat.size <= 2 * 1024 * 1024) result.push(absolute);
    }
  };
  visit(root);
  return result.sort();
}

function tokens(text) { return Math.ceil(String(text).length / 4); }

function runTask(repo, task, anchor) {
  const files = filesUnder(repo);
  const grepText = files.map((file) => readFileSync(file, "utf8")).filter((text) => text.toLowerCase().includes(task.toLowerCase())).join("\n");
  const fullPack = files.map((file) => `\n// ${relative(repo, file)}\n${readFileSync(file, "utf8")}`).join("\n");
  let neighborhood = "";
  try {
    neighborhood = execFileSync(process.execPath, [join(ROOT, "scripts/blueprint.mjs"), "neighborhood", anchor, "--budget-tokens", "800", "--json"], { cwd: repo, encoding: "utf8", timeout: 5000 });
  } catch (error) {
    neighborhood = String(error.stdout ?? "");
  }
  return {
    task,
    anchor,
    grep: { tokens: tokens(grepText), correct: grepText.length > 0 },
    fullPack: { tokens: tokens(fullPack), correct: fullPack.length > 0 },
    blueprintNeighborhood: { tokens: tokens(neighborhood), correct: neighborhood.length > 0 },
  };
}

const byRepo = new Map();
for (const [repoName] of TASKS) if (!byRepo.has(repoName)) byRepo.set(repoName, mkdtempSync(join(tmpdir(), "blueprint-baseline-")));
const results = [];
try {
  for (const [repoName, repo] of byRepo) {
    const source = join(FIXTURES, repoName);
    if (!readdirSync(repo).length) cpSync(source, repo, { recursive: true });
    execFileSync(process.execPath, [join(ROOT, "scripts/blueprint.mjs"), "build", "--out", ".agent"], { cwd: repo, stdio: "ignore", timeout: 120000 });
  }
  for (const [repoName, task, anchor] of TASKS) {
    const repo = byRepo.get(repoName);
    results.push(runTask(repo, task, anchor));
  }
  const report = { schemaVersion: 1, generatedAt: new Date().toISOString(), tasks: results };
  for (const repo of byRepo.values()) {
    const dbPath = join(repo, ".agent", "graph", "graph.db");
    if (!statSync(dbPath, { throwIfNoEntry: false })) continue;
    const db = openStore(dbPath);
    try { recordTokenSavings(db, report); } finally { closeStore(db); }
  }
  console.log(JSON.stringify(report, null, 2));
} finally {
  for (const repo of byRepo.values()) rmSync(repo, { recursive: true, force: true });
}
