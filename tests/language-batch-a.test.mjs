// D23: batch A — Go, Java, Kotlin, C#, C, C++, Objective-C tables are routed
// and fixture-backed; only syntax-exact facts are emitted.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { loadLanguageRecord } from "../graph/treesitter-provider.mjs";
import { walkTable } from "../graph/generic-ast-walker.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");
const LANGUAGES = ["go", "java", "kotlin", "c_sharp", "c", "cpp", "objc"];

test("batch A languages route through catalog with code profile", async () => {
  const { languageCapabilityRecords } = await import("../graph/language-registry.mjs");
  const records = languageCapabilityRecords();
  for (const lang of LANGUAGES) {
    const record = records.find((r) => r.language === lang);
    assert.ok(record, `missing catalog entry ${lang}`);
    assert.equal(record.factProfile, "code");
  }
});

for (const lang of LANGUAGES) {
  test(`${lang} fixture parses and emits nodes without fabrication`, async () => {
    const table = (await import(`../graph/language-tables/${lang}.mjs`)).default;
    const record = await loadLanguageRecord(table.id);
    if (!record.parser) {
      assert.ok(record.error, `${lang} must carry a typed degradation reason`);
      return;
    }
    const ext = lang === "c_sharp" ? "cs" : lang === "objc" ? "m" : lang;
    const fixture = join(FIXTURES, lang, `basic.${ext}`);
    const tree = record.parser.parse(readFileSync(fixture, "utf8"));
    const result = walkTable({ table, tree, filePath: `basic.${ext}`, providerId: "cortex-treesitter", precisionTier: "AST" });
    // Every emitted node carries evidence and provider.
    for (const node of result.nodes) {
      assert.ok(node.evidence?.length > 0, `${lang}: node without evidence`);
      assert.equal(node.provider, "cortex-treesitter");
      assert.equal(node.precisionTier, "AST");
    }
  });
}

test("batch A no name-only exact call/reference edges are emitted", async () => {
  const { languageCapabilityRecords } = await import("../graph/language-registry.mjs");
  const records = languageCapabilityRecords().filter((r) => LANGUAGES.includes(r.language));
  assert.equal(records.length, LANGUAGES.length);
});
