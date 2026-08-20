// D20: grammar manifest — exact 36-file inventory, catalog routing, registry
// lookup, and lexical fallback for unknown extensions.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { inventoryGrammars } from "../scripts/grammars/inventory.mjs";
import { verifyGrammars } from "../scripts/grammars/verify.mjs";
import { languageByExtension, languageCapabilityRecords, manifestDigest } from "../src/graph/language-registry.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts", "cortex.mjs");

test("inventory finds exactly 36 grammars with hashes", () => {
  const inventory = inventoryGrammars();
  assert.equal(inventory.entries.length, 36);
  assert.equal(inventory.missing.length, 0);
  assert.equal(inventory.extra.length, 0);
  assert.equal(inventory.version, "0.1.13");
  for (const entry of inventory.entries) {
    assert.match(entry.sha256, /^[a-f0-9]{64}$/);
    assert.ok(entry.size > 0);
  }
});

test("catalog routes every grammar with a valid fact profile", () => {
  const result = verifyGrammars();
  assert.equal(result.ok, true, result.problems?.join("; "));
  assert.equal(result.count, 36);
});

test("manifest digest is stable and present", () => {
  const manifest = JSON.parse(readFileSync(join(ROOT, "grammars", "manifest.json"), "utf8"));
  assert.equal(manifest.schemaVersion, 1);
  assert.ok(manifest.digest);
  assert.equal(manifestDigest(), manifest.digest);
  assert.equal(manifest.grammars.length, 36);
});

test("registry routes known extensions and falls back lexically for unknown", () => {
  const js = languageByExtension("ts");
  assert.equal(js.language, "typescript");
  assert.equal(js.fallback, false);
  const unknown = languageByExtension("weirdxyz");
  assert.equal(unknown.fallback, true);
  assert.equal(unknown.precisionTier, "LEXICAL");
});

test("capability records cover all 36 languages with tier and limits", () => {
  const records = languageCapabilityRecords();
  assert.equal(records.length, 36);
  for (const record of records) {
    assert.ok(record.language);
    assert.ok(record.extensions.length > 0);
    assert.ok(["code", "data", "markup", "specification"].includes(record.factProfile));
    assert.ok(["AST", "LEXICAL"].includes(record.precisionTier));
  }
});

test("cortex languages --json works from the facade", () => {
  const result = spawnSync(process.execPath, [CLI, "languages", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.schemaVersion, 1);
  assert.ok(payload.digest);
  assert.equal(payload.languages.length, 36);
});
