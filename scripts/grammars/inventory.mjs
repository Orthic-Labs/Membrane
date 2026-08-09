#!/usr/bin/env node
// D20: grammar inventory — reads installed WASM files, hashes bytes, reads
// package version/license, and fails on missing or extra grammar files. The
// exact 36-file inventory (S-17) is the pinned truth.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const GRAMMAR_OUT = join(ROOT, "node_modules", "tree-sitter-wasms", "out");

const PINNED = Object.freeze([
  "tree-sitter-bash.wasm",
  "tree-sitter-c.wasm",
  "tree-sitter-c_sharp.wasm",
  "tree-sitter-cpp.wasm",
  "tree-sitter-css.wasm",
  "tree-sitter-dart.wasm",
  "tree-sitter-elisp.wasm",
  "tree-sitter-elixir.wasm",
  "tree-sitter-elm.wasm",
  "tree-sitter-embedded_template.wasm",
  "tree-sitter-go.wasm",
  "tree-sitter-html.wasm",
  "tree-sitter-java.wasm",
  "tree-sitter-javascript.wasm",
  "tree-sitter-json.wasm",
  "tree-sitter-kotlin.wasm",
  "tree-sitter-lua.wasm",
  "tree-sitter-objc.wasm",
  "tree-sitter-ocaml.wasm",
  "tree-sitter-php.wasm",
  "tree-sitter-python.wasm",
  "tree-sitter-ql.wasm",
  "tree-sitter-rescript.wasm",
  "tree-sitter-ruby.wasm",
  "tree-sitter-rust.wasm",
  "tree-sitter-scala.wasm",
  "tree-sitter-solidity.wasm",
  "tree-sitter-swift.wasm",
  "tree-sitter-systemrdl.wasm",
  "tree-sitter-tlaplus.wasm",
  "tree-sitter-toml.wasm",
  "tree-sitter-tsx.wasm",
  "tree-sitter-typescript.wasm",
  "tree-sitter-vue.wasm",
  "tree-sitter-yaml.wasm",
  "tree-sitter-zig.wasm",
]);

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function packageVersion() {
  const pkg = JSON.parse(readFileSync(join(ROOT, "node_modules", "tree-sitter-wasms", "package.json"), "utf8"));
  return pkg.version ?? "unknown";
}

export function inventoryGrammars() {
  if (!existsSync(GRAMMAR_OUT)) throw new Error("tree-sitter-wasms/out missing; run pnpm install");
  const actual = readdirSync(GRAMMAR_OUT).filter((name) => name.endsWith(".wasm")).sort();
  const expected = [...PINNED].sort();
  const missing = expected.filter((name) => !actual.includes(name));
  const extra = actual.filter((name) => !expected.includes(name));
  const entries = expected.map((name) => {
    const path = join(GRAMMAR_OUT, name);
    return { file: name, sha256: sha256File(path), size: statSync(path).size, version: packageVersion() };
  });
  return { entries, missing, extra, version: packageVersion(), digest: createHash("sha256").update(entries.map((e) => `${e.file}:${e.sha256}`).join("\n")).digest("hex") };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try {
    const result = inventoryGrammars();
    if (process.argv.includes("--check")) {
      const problems = [...result.missing.map((m) => `missing ${m}`), ...result.extra.map((e) => `extra ${e}`)];
      if (problems.length) {
        console.error(problems.join("\n"));
        process.exit(1);
      }
      console.log(`inventory OK: ${result.entries.length} grammars, version ${result.version}, digest ${result.digest.slice(0, 16)}…`);
    } else {
      console.log(JSON.stringify(result, null, 2));
    }
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
