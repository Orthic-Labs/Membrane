#!/usr/bin/env node
/**
 * Generate Membrane's Rust CLI projection from the canonical operations index.
 *
 * Default mode writes stdout. `--write` updates the checked-in projection;
 * `--check` compares it without mutation. Build scripts use `--check`.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const root = resolve(scriptDir, "../../../..");
const indexPath = resolve(root, "schemas/registry/operations/operations-index.v1.golden.json");
const generatedPath = resolve(root, "engine/crates/membrane/src/generated_cli_subcommands.rs");
const args = new Set(process.argv.slice(2));

const index = JSON.parse(readFileSync(indexPath, "utf8"));
if (!Array.isArray(index.operations)) throw new Error("operations index has no operations array");
const ids = index.operations.map((entry) => {
  if (typeof entry?.name !== "string" || !entry.name) throw new Error("operation has no name");
  return entry.name;
});
if (new Set(ids).size !== ids.length) throw new Error("operations index contains duplicate names");
const digest = `sha256:${createHash("sha256").update(JSON.stringify(ids)).digest("hex")}`;
const cliName = (id) => id.toLowerCase().replaceAll(".", "-");

let source = `// GENERATED — DO NOT EDIT\n// operation_registry_version: ${digest}\nmatch name {\n`;
source += "    \"\" => Some(vec![\n";
for (const id of ids) source += `        (${JSON.stringify(cliName(id))}.to_string(), String::new()),\n`;
source += "    ]),\n";
for (const id of ids) {
  source += `    ${JSON.stringify(cliName(id))} => Some(vec![\n`;
  source += "    ]),\n";
}
source += "    _ => None,\n}\n";

if (args.has("--write")) {
  writeFileSync(generatedPath, source, "utf8");
} else if (args.has("--check")) {
  const actual = readFileSync(generatedPath, "utf8");
  if (actual !== source) {
    console.error(`generated CLI projection drifted: ${generatedPath}`);
    process.exitCode = 1;
  }
} else {
  process.stdout.write(source);
}
