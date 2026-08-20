// D24: batch B2 — separate worker bounds retained WASM grammar memory.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { loadLanguageRecord } from "../src/graph/treesitter-provider.mjs";
import { walkTable } from "../src/graph/generic-ast-walker.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");
const LANGUAGES = ["lua", "ocaml", "elm", "rescript", "solidity", "zig"];
const EXTENSIONS = { ocaml: "ml", rescript: "res", solidity: "sol" };

test("batch B2 languages route through catalog with code profile", async () => {
  const { languageCapabilityRecords } = await import("../src/graph/language-registry.mjs");
  const records = languageCapabilityRecords();
  for (const lang of LANGUAGES) {
    const record = records.find((candidate) => candidate.language === lang);
    assert.ok(record, `missing catalog entry ${lang}`);
    assert.equal(record.factProfile, "code");
  }
});

for (const lang of LANGUAGES) {
  test(`${lang} fixture parses with evidence-bearing nodes`, async () => {
    const table = (await import(`../src/graph/language-tables/${lang}.mjs`)).default;
    const record = await loadLanguageRecord(table.id);
    if (!record.parser) {
      assert.ok(record.error, `${lang} must carry a typed degradation reason`);
      return;
    }
    const extension = EXTENSIONS[lang] ?? lang;
    const fixture = join(FIXTURES, lang, `basic.${extension}`);
    const tree = record.parser.parse(readFileSync(fixture, "utf8"));
    try {
      const result = walkTable({ table, tree, filePath: `basic.${extension}`, providerId: "cortex-treesitter", precisionTier: "AST" });
      for (const node of result.nodes) {
        assert.ok(node.evidence?.length > 0, `${lang}: node without evidence`);
      }
    } finally {
      tree.delete();
    }
  });
}
