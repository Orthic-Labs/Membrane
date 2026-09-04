import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  diffFiles,
  extractorFingerprintForPath,
  loadParseCache,
  nextCache,
  PARSE_CACHE_VERSION,
  writeParseCache,
} from "../src/graph/parse-cache.mjs";

test("parse cache records carry an automatic semantic extractor fingerprint", () => {
  const outDir = mkdtempSync(join(tmpdir(), "blueprint-parse-cache-fingerprint-"));
  try {
    const cache = nextCache([{
      path: "src/example.ts",
      record: { contentHash: "same-bytes", symbols: [{ id: "symbol:example" }] },
    }]);
    writeParseCache(outDir, cache);

    const stored = JSON.parse(readFileSync(join(outDir, "graph", "parse-cache", "records.json"), "utf8"));
    assert.equal(stored.version, PARSE_CACHE_VERSION);
    assert.equal(
      stored.records["src/example.ts"].extractorFingerprint,
      extractorFingerprintForPath("src/example.ts"),
    );

    const loaded = loadParseCache(outDir);
    assert.equal(loaded.records.size, 1);
    const diff = diffFiles([{ path: "src/example.ts", contentHash: "same-bytes" }], loaded);
    assert.equal(diff.reused.length, 1);
    assert.equal(diff.changed.length, 0);
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
});

test("unchanged source is reparsed when semantic extractor inputs change", () => {
  const outDir = mkdtempSync(join(tmpdir(), "blueprint-parse-cache-semantic-change-"));
  try {
    const cache = nextCache([{
      path: "src/example.ts",
      record: { contentHash: "same-bytes", symbols: [{ id: "symbol:old-extraction" }] },
    }]);
    writeParseCache(outDir, cache);

    const changedSemantics = loadParseCache(outDir, { semanticSalt: "simulated-extractor-fix" });
    assert.equal(
      changedSemantics.records.size,
      0,
      "byte-identical records from old extractor semantics must not survive",
    );
    const diff = diffFiles(
      [{ path: "src/example.ts", contentHash: "same-bytes" }],
      changedSemantics,
      { semanticSalt: "simulated-extractor-fix" },
    );
    assert.equal(diff.reused.length, 0);
    assert.equal(diff.changed.length, 1);
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
});

test("fingerprint is scoped by source kind as well as extractor inputs", () => {
  assert.notEqual(
    extractorFingerprintForPath("src/example.ts"),
    extractorFingerprintForPath("src/example.py"),
  );
});
