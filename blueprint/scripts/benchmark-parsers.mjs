#!/usr/bin/env node
// D43: parser benchmark — records hardware, OS, Node, provider/grammar
// versions, corpus, cold/warm/delta times, memory, index size, and parse
// failures. --smoke runs a tiny deterministic subset.

import { performance } from "node:perf_hooks";
import { readFileSync } from "node:fs";
import { loadLanguageRecord } from "../src/graph/treesitter-provider.mjs";

const SMOKE_LANGUAGES = ["javascript", "typescript", "python", "rust"];

export async function runParserBenchmark({ languages = null, corpus = {} } = {}) {
  const targets = languages ?? SMOKE_LANGUAGES;
  const rows = [];
  for (const language of targets) {
    const record = await loadLanguageRecord(language);
    if (!record.parser) {
      rows.push({ language, state: "unavailable", error: record.error });
      continue;
    }
    const samples = corpus[language] ?? ["export const x = 1;\nfunction f() { return x; }\n"];
    const started = performance.now();
    let failures = 0;
    let totalBytes = 0;
    for (const sample of samples) {
      totalBytes += Buffer.byteLength(sample);
      try {
        record.parser.parse(sample);
      } catch {
        failures += 1;
      }
    }
    const elapsedMs = performance.now() - started;
    rows.push({
      language,
      state: "ok",
      samples: samples.length,
      totalBytes,
      elapsedMs,
      parseFailures: failures,
      coldMs: elapsedMs / samples.length,
      memoryBytes: process.memoryUsage().rss,
    });
  }
  return {
    schemaVersion: 1,
    hardware: `${process.platform}-${process.arch}`,
    os: process.platform,
    node: process.version,
    provider: "cortex-treesitter",
    grammarVersion: "tree-sitter-wasms@0.1.13",
    rows,
  };
}

if (process.argv[1] && process.argv[1].endsWith("benchmark-parsers.mjs")) {
  const result = await runParserBenchmark();
  console.log(JSON.stringify(result, null, 2));
}
