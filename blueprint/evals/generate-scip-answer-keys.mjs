#!/usr/bin/env node
import { createXXHash128 } from "hash-wasm";

// XXH3-128, matching production. The harness previously hashed fixtures with
// sha256 while the providers emitted xxh128, and that impedance mismatch is
// exactly what silently failed every qualification task after 3988a735.
const xxhasher = await createXXHash128();

function xxh3Hex(bytes) {
  xxhasher.init();
  xxhasher.update(bytes);
  return xxhasher.digest("hex");
}

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";


function args(argv) {
  const out = { indexes: {} };
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    const value = argv[++index];
    if (key === "--index") {
      const split = value.indexOf("=");
      if (split < 1) throw new Error("--index must be repo=path");
      out.indexes[value.slice(0, split)] = path.resolve(value.slice(split + 1));
    } else if (key.startsWith("--")) out[key.slice(2)] = value;
    else throw new Error(`unexpected argument: ${key}`);
  }
  return out;
}


function sha256(file) {
  return xxh3Hex(readFileSync(file));
}


function normalize(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}


function wslPath(file) {
  const absolute = path.resolve(file);
  const match = absolute.match(/^([A-Za-z]):\\(.*)$/);
  if (!match) throw new Error(`cannot translate Windows path to WSL: ${absolute}`);
  return `/mnt/${match[1].toLowerCase()}/${match[2].replaceAll("\\", "/")}`;
}


function printIndex(cli, index) {
  const command = process.platform === "win32" ? "wsl.exe" : cli;
  const commandArgs = process.platform === "win32"
    ? [wslPath(cli), "print", "--json", wslPath(index)]
    : ["print", "--json", index];
  const result = spawnSync(command, commandArgs, { encoding: "utf8", timeout: 30000, maxBuffer: 64 * 1024 * 1024, windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`scip print failed for ${index}: ${result.stderr}`);
  return JSON.parse(result.stdout);
}


function loadTasks(file) {
  return readFileSync(file, "utf8").split(/\r?\n/).filter((line) => line.trim()).map(JSON.parse);
}


function occurrenceLines(range) {
  if (range.length === 3) return [range[0] + 1, range[0] + 1];
  return [range[0] + 1, range[2] + 1];
}


function taskAnswer(task, documents) {
  const evidence = [];
  for (const expected of task.expectedEvidence) {
    const document = documents.find((item) => normalize(item.relative_path) === normalize(expected.path));
    if (!document) continue;
    const occurrences = (document.occurrences ?? []).flatMap((occurrence) => {
      const [startLine, endLine] = occurrenceLines(occurrence.range);
      if (endLine < expected.startLine || startLine > expected.endLine || !occurrence.symbol) return [];
      return [{
        symbol: occurrence.symbol,
        role: (occurrence.symbol_roles & 1) === 1 ? "definition" : "reference",
        startLine,
        endLine,
      }];
    });
    evidence.push({ path: normalize(expected.path), expectedSpan: [expected.startLine, expected.endLine], occurrences });
  }
  const symbols = [...new Set(evidence.flatMap((item) => item.occurrences.map((occurrence) => occurrence.symbol)))].sort();
  return { symbols, evidence };
}


function main() {
  const options = args(process.argv.slice(2));
  if (!options.tasks || !options.out || !options["scip-cli"]) throw new Error("required: --tasks --out --scip-cli --index repo=path");
  const tasks = loadTasks(path.resolve(options.tasks));
  const indexes = {};
  for (const [repo, index] of Object.entries(options.indexes)) {
    indexes[repo] = { path: index, sha256: sha256(index), value: printIndex(options["scip-cli"], index) };
  }
  const answers = {};
  for (const task of tasks.filter((item) => item.oracle?.status === "pending")) {
    const index = indexes[task.repo];
    if (!index) throw new Error(`${task.id}: no SCIP index for ${task.repo}`);
    const answer = taskAnswer(task, index.value.documents ?? []);
    if (!answer.symbols.length) throw new Error(`${task.id}: expected spans contain no SCIP occurrences`);
    answers[task.id] = answer;
  }
  const artifact = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    generators: {
      scipTypescript: "0.4.0",
      rustAnalyzer: "1.96.1 (31fca3ad 2026-06-26)",
      scipPython: "0.6.6",
      scipCli: "0.9.0",
    },
    indexes: Object.fromEntries(Object.entries(indexes).map(([repo, item]) => [repo, { sha256: item.sha256 }])),
    tasks: answers,
  };
  writeFileSync(path.resolve(options.out), `${JSON.stringify(artifact, null, 2)}\n`);
  console.log(JSON.stringify({ out: path.resolve(options.out), tasks: Object.keys(answers).length, indexes: Object.keys(indexes).length }));
}


main();
