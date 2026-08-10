// D24: batch B3 — separate worker bounds retained WASM grammar memory.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { loadLanguageRecord } from "../graph/treesitter-provider.mjs";
import { walkTable } from "../graph/generic-ast-walker.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");
const LANGUAGES = ["dart", "scala", "elixir"];

test("batch B3 languages route through catalog with code profile", async () => {
  const { languageCapabilityRecords } = await import("../graph/language-registry.mjs");
  const records = languageCapabilityRecords();
  for (const lang of LANGUAGES) {
    const record = records.find((candidate) => candidate.language === lang);
    assert.ok(record, `missing catalog entry ${lang}`);
    assert.equal(record.factProfile, "code");
  }
});

for (const lang of LANGUAGES) {
  test(`${lang} fixture parses with evidence-bearing nodes`, async () => {
    const table = (await import(`../graph/language-tables/${lang}.mjs`)).default;
    const record = await loadLanguageRecord(table.id);
    if (!record.parser) {
      assert.ok(record.error, `${lang} must carry a typed degradation reason`);
      return;
    }
    const fixture = join(FIXTURES, lang, `basic.${lang}`);
    const tree = record.parser.parse(readFileSync(fixture, "utf8"));
    const result = walkTable({ table, tree, filePath: `basic.${lang}`, providerId: "cortex-treesitter", precisionTier: "AST" });
    for (const node of result.nodes) {
      assert.ok(node.evidence?.length > 0, `${lang}: node without evidence`);
    }
  });
}
