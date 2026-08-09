// D24: batch B — PHP, Ruby, Swift, Dart, Scala, Elixir, Lua, OCaml, Elm,
// ReScript, Solidity, Zig tables are routed and honest (dynamic ambiguity
// visible, no fabricated facts).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { loadLanguageRecord } from "../graph/treesitter-provider.mjs";
import { walkTable } from "../graph/generic-ast-walker.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");
const LANGUAGES = ["php", "ruby", "swift", "dart", "scala", "elixir", "lua", "ocaml", "elm", "rescript", "solidity", "zig"];

test("batch B languages route through catalog with code profile", async () => {
  const { languageCapabilityRecords } = await import("../graph/language-registry.mjs");
  const records = languageCapabilityRecords();
  for (const lang of LANGUAGES) {
    const record = records.find((r) => r.language === lang);
    assert.ok(record, `missing catalog entry ${lang}`);
    assert.equal(record.factProfile, "code");
  }
});

for (const lang of LANGUAGES) {
  test(`${lang} fixture parses with evidence-bearing nodes`, async () => {
    const table = (await import(`../graph/language-tables/${lang}.mjs`)).default;
    const record = await loadLanguageRecord(table.id);
    // ABI-incompatible grammars degrade honestly (typed error), never fabricate.
    if (!record.parser) {
      assert.ok(record.error, `${lang} must carry a typed degradation reason`);
      return;
    }
    const ext = lang === "ruby" ? "rb" : lang;
    const fixture = join(FIXTURES, lang, `basic.${ext}`);
    const tree = record.parser.parse(readFileSync(fixture, "utf8"));
    const result = walkTable({ table, tree, filePath: `basic.${lang}`, providerId: "cortex-treesitter", precisionTier: "AST" });
    for (const node of result.nodes) {
      assert.ok(node.evidence?.length > 0, `${lang}: node without evidence`);
    }
  });
}

test("dynamic-language ambiguous calls stay unresolved", async () => {
  const table = (await import("../graph/language-tables/ruby.mjs")).default;
  const record = await loadLanguageRecord(table.id);
  const tree = record.parser.parse(readFileSync(join(FIXTURES, "ruby", "basic.rb"), "utf8"));
  const result = walkTable({ table, tree, filePath: "basic.rb", providerId: "cortex-treesitter", precisionTier: "AST" });
  // Ruby dynamic dispatch never fabricates a resolved CALLS target.
  for (const edge of result.edges.filter((e) => e.kind === "CALLS")) {
    assert.equal(edge.confidenceTier, "UNRESOLVED");
  }
});
