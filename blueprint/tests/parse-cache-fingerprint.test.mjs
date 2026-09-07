import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  cacheIdentity,
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

test("cache identity is checked before any record payload is hydrated", () => {
  const outDir = mkdtempSync(join(tmpdir(), "blueprint-parse-cache-identity-"));
  try {
    writeParseCache(outDir, nextCache([{
      path: "src/example.ts",
      record: { contentHash: "same-bytes", symbols: [{ id: "symbol:example" }] },
    }]));
    const file = join(outDir, "graph", "parse-cache", "records.json");
    const stored = JSON.parse(readFileSync(file, "utf8"));
    assert.equal(stored.identity, cacheIdentity(), "the cache header must carry a path-independent identity");

    // Poison every record payload. If identity were checked per record — after
    // hydration — this would be reached; a header check must reject first.
    stored.records = { "src/a.ts": null, "src/b.ts": "not-an-object", "src/c.ts": { extractorFingerprint: "x" } };
    stored.identity = "identity-from-different-extractor-semantics";
    writeFileSync(file, JSON.stringify(stored));
    assert.equal(loadParseCache(outDir).records.size, 0);

    // Identity moves with provider identity, not only with source bytes.
    assert.notEqual(cacheIdentity(), cacheIdentity({ providerVersion: "9.9.9" }));
    assert.notEqual(cacheIdentity(), cacheIdentity({ semanticSalt: "extractor-fix" }));
    // ...and unlike the per-path fingerprint, it does not vary by file kind.
    assert.equal(cacheIdentity(), cacheIdentity({}));
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
});

test("a narrow read hydrates only what was asked for and preserves wider coverage", () => {
  const outDir = mkdtempSync(join(tmpdir(), "blueprint-parse-cache-coverage-"));
  try {
    const wide = nextCache([
      { path: "src/a.ts", record: { contentHash: "a1", symbols: [{ id: "symbol:a" }] } },
      { path: "src/b.ts", record: { contentHash: "b1", symbols: [{ id: "symbol:b" }] } },
      { path: "src/c.ts", record: { contentHash: "c1", symbols: [{ id: "symbol:c" }] } },
    ]);
    writeParseCache(outDir, wide);

    // Narrow request: one file. Bounded work — the other two are never hydrated.
    const narrow = loadParseCache(outDir, {}, { paths: ["src/b.ts"] });
    assert.deepEqual([...narrow.records.keys()], ["src/b.ts"]);
    assert.deepEqual([...narrow.retained.keys()].sort(), ["src/a.ts", "src/c.ts"]);

    // Cache-transparent: the narrow answer equals the wide one for that file.
    const fromWide = loadParseCache(outDir);
    assert.deepEqual(narrow.records.get("src/b.ts"), fromWide.records.get("src/b.ts"));

    // Republishing after the narrow request must not shrink coverage.
    const republished = nextCache(
      [{ path: "src/b.ts", record: { contentHash: "b2", symbols: [{ id: "symbol:b2" }] } }],
      {},
      { retainFrom: narrow },
    );
    writeParseCache(outDir, republished);
    const reloaded = loadParseCache(outDir);
    assert.deepEqual([...reloaded.records.keys()].sort(), ["src/a.ts", "src/b.ts", "src/c.ts"]);
    assert.equal(reloaded.records.get("src/b.ts").contentHash, "b2", "the narrow write still wins for its own path");
    assert.equal(reloaded.records.get("src/a.ts").contentHash, "a1", "untouched coverage is preserved verbatim");

    // A same-byte no-op over the untouched files is a warm hit, not a reparse.
    const diff = diffFiles(
      [{ path: "src/a.ts", contentHash: "a1" }, { path: "src/c.ts", contentHash: "c1" }],
      reloaded,
    );
    assert.equal(diff.reused.length, 2);
    assert.equal(diff.changed.length, 0);
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
});

test("a corrupt or identity-less cache file fails closed", () => {
  const outDir = mkdtempSync(join(tmpdir(), "blueprint-parse-cache-corrupt-"));
  try {
    writeParseCache(outDir, nextCache([{ path: "src/a.ts", record: { contentHash: "a1" } }]));
    const file = join(outDir, "graph", "parse-cache", "records.json");
    const stored = JSON.parse(readFileSync(file, "utf8"));

    writeFileSync(file, "{ this is not json");
    assert.equal(loadParseCache(outDir).records.size, 0);

    delete stored.identity;
    writeFileSync(file, JSON.stringify(stored));
    assert.equal(loadParseCache(outDir).records.size, 0, "a cache with no identity claim is not usable");
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
});
