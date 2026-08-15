import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const read = (relative) => readFileSync(path.join(here, relative), "utf8");

function numeric(raw) {
  const expr = raw.trim();
  assert.match(expr, /^[0-9_]+(\s*\*\s*[0-9_]+)*$/);
  return expr
    .split("*")
    .map((part) => Number(part.replace(/_/g, "").trim()))
    .reduce((product, n) => product * n, 1);
}

function extract(source, pattern, wanted) {
  const constants = {};
  let match;
  while ((match = pattern.exec(source)) !== null) {
    if (wanted.includes(match[1])) constants[match[1]] = numeric(match[2]);
  }
  return constants;
}

const PAIRS = [
  ["MAX_ITEMS", "CODE_BATCH_MAX_ITEMS"],
  ["MAX_BYTES", "CODE_BATCH_MAX_BYTES"],
  ["MAX_TOKENS", "CODE_BATCH_MAX_TOKENS"],
  ["MAX_DEADLINE_MS", "CODE_BATCH_MAX_DEADLINE_MS"],
];

test("Rust and JS code-batch limits remain equal", () => {
  const wantedRust = ["MAX_ITEMS", "MAX_BYTES", "MAX_TOKENS", "MAX_DEADLINE_MS"];
  const wantedJs = ["CODE_BATCH_MAX_ITEMS", "CODE_BATCH_MAX_BYTES", "CODE_BATCH_MAX_TOKENS", "CODE_BATCH_MAX_DEADLINE_MS"];
  const rust = extract(read("../../engine/crates/membrane-runtime/src/code_batch.rs"), /pub const (\w+): \w+ = ([^;]+);/g, wantedRust);
  const js = extract(read("../../operations/code/code-batch-limits.mjs"), /export const (\w+) = ([^;]+);/g, wantedJs);
  for (const [rustName, jsName] of PAIRS) {
    assert.ok(rustName in rust, `code_batch.rs is missing ${rustName}`);
    assert.ok(jsName in js, `code-batch-limits.mjs is missing ${jsName}`);
    assert.equal(
      js[jsName],
      rust[rustName],
      `${jsName} (${js[jsName]}) diverges from ${rustName} (${rust[rustName]})`,
    );
  }
});
