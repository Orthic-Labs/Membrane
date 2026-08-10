// D22: treesitter parity — legacy vs generic engines over the pinned JS
// fixture. Semantic node/edge sets, evidence spans, parse reports, and
// provider identity must match (or the table/walker is fixed, never the
// golden weakened).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test, { after } from "node:test";

import { disposeTreeSitterProvider, extractFile, genericEngineEnabled } from "../graph/treesitter-provider.mjs";
import javascriptTable from "../graph/language-tables/javascript.mjs";
import typescriptTable from "../graph/language-tables/typescript.mjs";
import tsxTable from "../graph/language-tables/tsx.mjs";
import pythonTable from "../graph/language-tables/python.mjs";
import rustTable from "../graph/language-tables/rust.mjs";

const FIXTURES = join(import.meta.dirname, "fixtures", "languages");

after(() => disposeTreeSitterProvider());

function canonicalize(result) {
  const canonical = (value) => JSON.parse(JSON.stringify(value));
  const legacyFields = ({ provider, grammarHash, precisionTier, confidenceTier, ...value }) => value;
  return {
    path: result.path,
    language: result.language,
    parseStatus: result.parseStatus,
    errorNodeCount: result.errorNodeCount,
    nodes: canonical((result.nodes ?? []).map(legacyFields)),
    edges: canonical((result.edges ?? []).map(legacyFields)),
    rawImports: canonical((result.rawImports ?? []).map(legacyFields)),
    rawCalls: canonical((result.rawCalls ?? []).map(legacyFields)),
    configuresTargets: (result.configuresTargets ?? []).map((target) => ({
      name: target.name,
      value: target.value,
      span: target.node ? [target.node.startPosition.row, target.node.endPosition.row] : null,
    })),
  };
}

async function legacyExtract(filePath, table) {
  const previous = process.env.CORTEX_AST_ENGINE;
  process.env.CORTEX_AST_ENGINE = "legacy";
  try {
    const result = await extractFile({ path: filePath, text: readFileSync(filePath, "utf8") });
    return canonicalize(result);
  } finally {
    if (previous === undefined) delete process.env.CORTEX_AST_ENGINE;
    else process.env.CORTEX_AST_ENGINE = previous;
  }
}

async function genericExtract(filePath) {
  const previous = process.env.CORTEX_AST_ENGINE;
  process.env.CORTEX_AST_ENGINE = "generic";
  try {
    const result = await extractFile({ path: filePath, text: readFileSync(filePath, "utf8") });
    assert.equal(result.generic, true, "production generic path must stay selected");
    assert.ok(result.nodes.slice(1).every((node) => node.provider === "cortex-treesitter" && node.grammarHash && node.precisionTier === "AST"));
    assert.ok(result.edges.every((edge) => edge.provider === "cortex-treesitter" && edge.grammarHash && edge.precisionTier === "AST"));
    return canonicalize(result);
  } finally {
    if (previous === undefined) delete process.env.CORTEX_AST_ENGINE;
    else process.env.CORTEX_AST_ENGINE = previous;
  }
}

test("generic engine is selected by default", () => {
  const previous = process.env.CORTEX_AST_ENGINE;
  delete process.env.CORTEX_AST_ENGINE;
  try {
    assert.equal(genericEngineEnabled(), true);
  } finally {
    if (previous === undefined) delete process.env.CORTEX_AST_ENGINE;
    else process.env.CORTEX_AST_ENGINE = previous;
  }
});

for (const [name, table, fixture] of [
  ["javascript", javascriptTable, "javascript/basic.js"],
  ["typescript", typescriptTable, "javascript/basic.ts"],
  ["python", pythonTable, "javascript/basic.py"],
  ["rust", rustTable, "javascript/basic.rs"],
]) {
  test(`${name} legacy and generic engines agree on full canonical projection`, async () => {
    const filePath = join(FIXTURES, fixture);
    const legacy = await legacyExtract(filePath, table);
    const generic = await genericExtract(filePath);
    if (name === "javascript" || name === "typescript") {
      assert.ok(generic.edges.some((edge) => edge.kind === "CONTAINS"), `${name}: production generic extraction must emit CONTAINS`);
    }
    assert.deepEqual(generic, legacy, `${name}: generic canonical projection drifted from compatibility mode`);
  });
}

test("tsx legacy and generic engines agree on full canonical projection", async () => {
  const file = { path: "src/Widget.tsx", text: "import React from 'react'; export interface Props { title: string } export function Widget({ title }: Props) { return <main>{title}</main>; }" };
  const previous = process.env.CORTEX_AST_ENGINE;
  try {
    process.env.CORTEX_AST_ENGINE = "legacy";
    const legacy = canonicalize(await extractFile(file));
    process.env.CORTEX_AST_ENGINE = "generic";
    const generic = canonicalize(await extractFile(file));
    assert.equal(tsxTable.id, "tsx");
    assert.deepEqual(generic, legacy);
  } finally {
    if (previous === undefined) delete process.env.CORTEX_AST_ENGINE;
    else process.env.CORTEX_AST_ENGINE = previous;
  }
});
