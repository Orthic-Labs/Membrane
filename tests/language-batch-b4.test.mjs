// D24: batch B4 — Swift uses its own worker to release its large WASM grammar.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { loadLanguageRecord } from "../graph/treesitter-provider.mjs";
import { walkTable } from "../graph/generic-ast-walker.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");

test("Swift routes through catalog with code profile", async () => {
  const { languageCapabilityRecords } = await import("../graph/language-registry.mjs");
  const record = languageCapabilityRecords().find((candidate) => candidate.language === "swift");
  assert.ok(record, "missing catalog entry swift");
  assert.equal(record.factProfile, "code");
});

test("swift fixture parses with evidence-bearing nodes", async () => {
  const table = (await import("../graph/language-tables/swift.mjs")).default;
  const record = await loadLanguageRecord(table.id);
  if (!record.parser) {
    assert.ok(record.error, "swift must carry a typed degradation reason");
    return;
  }
  const fixture = join(FIXTURES, "swift", "basic.swift");
  const tree = record.parser.parse(readFileSync(fixture, "utf8"));
  try {
    const result = walkTable({ table, tree, filePath: "basic.swift", providerId: "cortex-treesitter", precisionTier: "AST" });
    for (const node of result.nodes) {
      assert.ok(node.evidence?.length > 0, "swift: node without evidence");
    }
  } finally {
    tree.delete();
  }
});
