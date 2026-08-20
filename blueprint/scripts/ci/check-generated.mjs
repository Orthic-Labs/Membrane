#!/usr/bin/env node
// D10: generated-file drift check. Regenerates the human docs from the
// current graph and fails if any checked-in generated artifact changed.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const GENERATED = ["docs/product.md", "docs/architecture.md"];

const missing = GENERATED.filter((path) => !existsSync(join(ROOT, path)));
if (missing.length) {
  console.error(`check-generated: missing generated docs: ${missing.join(", ")}`);
  process.exit(1);
}

const before = new Map(GENERATED.map((path) => [path, readFileSync(join(ROOT, path), "utf8")]));

const build = spawnSync(process.execPath, [join(ROOT, "scripts", "blueprint.mjs"), "build", "--out", ".agent", "--no-readme-link"], {
  cwd: ROOT,
  encoding: "utf8",
  timeout: 300000,
});
if (build.status !== 0) {
  console.error(`check-generated: build failed: ${build.stderr || build.stdout}`);
  process.exit(1);
}

const drifted = GENERATED.filter((path) => readFileSync(join(ROOT, path), "utf8") !== before.get(path));
if (drifted.length) {
  console.error(`check-generated: generated docs drifted after rebuild: ${drifted.join(", ")}`);
  for (const path of drifted) {
    const beforeLines = before.get(path).split(/\r?\n/);
    const afterLines = readFileSync(join(ROOT, path), "utf8").split(/\r?\n/);
    const firstDifference = beforeLines.findIndex((line, index) => line !== afterLines[index]);
    const lineIndex = firstDifference < 0 ? Math.min(beforeLines.length, afterLines.length) : firstDifference;
    console.error(`${path}:${lineIndex + 1}: expected ${JSON.stringify(beforeLines[lineIndex] ?? "")}`);
    console.error(`${path}:${lineIndex + 1}: rebuilt  ${JSON.stringify(afterLines[lineIndex] ?? "")}`);
  }
  console.error("Regenerate and commit them, or fix the generator.");
  process.exit(1);
}

console.log(`check-generated OK: ${GENERATED.join(", ")} byte-identical after rebuild`);
