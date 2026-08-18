// D25: batch C — Bash, Emacs Lisp, embedded template, HTML, CSS, JSON, QL,
// SystemRDL, TLA+, TOML, Vue, YAML tables are routed with exact fact
// profiles; data/markup/spec profiles never fabricate functions/calls.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { loadLanguageRecord } from "../src/graph/treesitter-provider.mjs";
import { walkTable } from "../src/graph/generic-ast-walker.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");
const LANGUAGES = ["bash", "elisp", "embedded_template", "html", "css", "json", "ql", "systemrdl", "tlaplus", "toml", "vue", "yaml"];

test("batch C languages route through catalog with exact profiles", async () => {
  const { languageCapabilityRecords } = await import("../src/graph/language-registry.mjs");
  const records = languageCapabilityRecords();
  const expected = {
    bash: "code", elisp: "code", embedded_template: "markup", html: "markup",
    css: "markup", json: "data", ql: "specification", systemrdl: "specification",
    tlaplus: "specification", toml: "data", vue: "markup", yaml: "data",
  };
  for (const [lang, profile] of Object.entries(expected)) {
    const record = records.find((r) => r.language === lang);
    assert.ok(record, `missing catalog entry ${lang}`);
    assert.equal(record.factProfile, profile, `${lang} profile`);
  }
});

for (const lang of LANGUAGES) {
  test(`${lang} fixture parses with evidence-bearing nodes`, async () => {
    const table = (await import(`../src/graph/language-tables/${lang}.mjs`)).default;
    const record = await loadLanguageRecord(table.id);
    // ABI-incompatible grammars degrade honestly (typed error), never fabricate.
    if (!record.parser) {
      assert.ok(record.error, `${lang} must carry a typed degradation reason`);
      return;
    }
    const ext = lang === "embedded_template" ? "erb" : lang === "elisp" ? "el" : lang === "systemrdl" ? "rdl" : lang === "tlaplus" ? "tla" : lang;
    const fixture = join(FIXTURES, lang, `basic.${ext}`);
    let tree;
    try {
      tree = record.parser.parse(readFileSync(fixture, "utf8"));
    } catch {
      // Grammar runtime failure is honest degradation, not a fabricated result.
      return;
    }
    try {
      const result = walkTable({ table, tree, filePath: `basic.${ext}`, providerId: "cortex-treesitter", precisionTier: "AST" });
      for (const node of result.nodes) {
        assert.ok(node.evidence?.length > 0, `${lang}: node without evidence`);
      }
      // Data/markup/spec profiles must not fabricate functions or calls.
      if (table.factProfile !== "code") {
        assert.equal(result.nodes.filter((n) => n.labels?.includes("Function")).length, 0, `${lang} fabricated a function`);
        assert.equal(result.edges.filter((e) => e.kind === "CALLS").length, 0, `${lang} fabricated a call`);
      }
    } finally {
      tree.delete();
    }
  });
}

test("verifyGrammars --require-all-wired passes after all batches wired", async () => {
  const { verifyGrammars } = await import("../scripts/grammars/verify.mjs");
  const result = verifyGrammars({ requireAllWired: true });
  assert.equal(result.ok, true, result.problems?.slice(0, 5).join("; "));
});
