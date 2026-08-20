import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";


const CORTEX = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TASKS = readFileSync(path.join(CORTEX, "evals/graph-tasks.jsonl"), "utf8")
  .split(/\r?\n/).filter(Boolean).map(JSON.parse);
const ANSWERS = JSON.parse(readFileSync(path.join(CORTEX, "evals/scip-answer-keys.json"), "utf8"));


test("every SCIP-authored task has a non-empty generated answer key", () => {
  const generated = TASKS.filter((task) => task.oracle?.kind?.startsWith("scip-") || task.oracle?.kind === "scip-typescript");
  for (const task of generated) {
    const answer = ANSWERS.tasks[task.id];
    assert.ok(answer, `missing ${task.id}`);
    assert.ok(answer.symbols.length > 0, `${task.id}: no symbols`);
    assert.ok(answer.evidence.some((item) => item.occurrences.length > 0), `${task.id}: no occurrences`);
  }
});


test("answer keys record all three index hashes and exact tool versions", () => {
  assert.deepEqual(Object.keys(ANSWERS.indexes).sort(), ["python-context", "rust-audio", "typescript-commerce"]);
  for (const value of Object.values(ANSWERS.indexes)) assert.match(value.sha256, /^[a-f0-9]{64}$/);
  assert.equal(ANSWERS.generators.scipTypescript, "0.4.0");
  assert.equal(ANSWERS.generators.scipPython, "0.6.6");
  assert.equal(ANSWERS.generators.scipCli, "0.9.0");
});
