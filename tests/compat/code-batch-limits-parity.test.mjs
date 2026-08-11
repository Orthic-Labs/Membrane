// MBR-402: proves engine/crates/membrane-runtime/src/code_batch.rs and
// operations/code/code-batch-limits.mjs (imported by operations/code/mbr402-batch.mjs)
// cannot silently diverge. code-batch-limits.mjs's own comment claims
// "single source of truth is code_batch.rs" but nothing enforced that claim
// before this test -- it extracts both sides' numeric literals from source
// text (no cargo build step, per this repair contract's D-3 preference for
// mechanism (b)) and asserts they are equal. Run: `node --test
// tests/compat/code-batch-limits-parity.test.mjs`.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const rustPath = path.join(here, "../../engine/crates/membrane-runtime/src/code_batch.rs");
const jsPath = path.join(here, "../../operations/code/code-batch-limits.mjs");

// Numeric literal grammar shared by both sides: digits, underscores as digit
// separators, and (JS only) a `*` multiplication of two such literals (e.g.
// `1024 * 1024`). Evaluating this narrow, digit/underscore/`*`/whitespace-only
// grammar is safe -- it can never contain anything but numerals.
function evalNumericLiteral(raw) {
  const expr = raw.trim();
  if (!/^[0-9_]+(\s*\*\s*[0-9_]+)*$/.test(expr)) {
    throw new Error(`unexpected non-numeric literal: ${raw}`);
  }
  return expr
    .split("*")
    .map((part) => Number(part.replace(/_/g, "").trim()))
    .reduce((product, n) => product * n, 1);
}

function extractRustConstants(source) {
  const constants = {};
  const re = /pub const (\w+): \w+ = ([^;]+);/g;
  let match;
  while ((match = re.exec(source)) !== null) {
    constants[match[1]] = evalNumericLiteral(match[2]);
  }
  return constants;
}

function extractJsConstants(source) {
  const constants = {};
  const re = /export const (\w+) = ([^;]+);/g;
  let match;
  while ((match = re.exec(source)) !== null) {
    constants[match[1]] = evalNumericLiteral(match[2]);
  }
  return constants;
}

// Rust name -> JS name mapping (JS side prefixes with CODE_BATCH_ per its own
// exported vocabulary; this pairing is the contract between the two files).
const PAIRS = [
  ["MAX_ITEMS", "CODE_BATCH_MAX_ITEMS"],
  ["MAX_BYTES", "CODE_BATCH_MAX_BYTES"],
  ["MAX_TOKENS", "CODE_BATCH_MAX_TOKENS"],
  ["MAX_DEADLINE_MS", "CODE_BATCH_MAX_DEADLINE_MS"],
];

test("code_batch.rs and code-batch-limits.mjs constants are extracted and present", () => {
  const rust = extractRustConstants(readFileSync(rustPath, "utf8"));
  const js = extractJsConstants(readFileSync(jsPath, "utf8"));
  for (const [rustName, jsName] of PAIRS) {
    assert.ok(rustName in rust, `code_batch.rs is missing ${rustName}`);
    assert.ok(jsName in js, `code-batch-limits.mjs is missing ${jsName}`);
  }
});

test("code_batch.rs and code-batch-limits.mjs constants agree numerically", () => {
  const rust = extractRustConstants(readFileSync(rustPath, "utf8"));
  const js = extractJsConstants(readFileSync(jsPath, "utf8"));
  for (const [rustName, jsName] of PAIRS) {
    assert.equal(
      js[jsName],
      rust[rustName],
      `${jsName} (${js[jsName]}) diverges from ${rustName} (${rust[rustName]})`,
    );
  }
});
