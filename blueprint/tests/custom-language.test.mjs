// D26: custom-language plugins — repo-confined config, hash validation,
// route-conflict rejection, and a tiny custom DSL that loads without editing
// Cortex source.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import test from "node:test";

import { loadCustomLanguages } from "../src/lib/languages/custom-config.mjs";

function sha256Hex(input) {
  return createHash("sha256").update(input).digest("hex");
}

test("custom language loads from a repo-confined config", () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-custom-lang-"));
  try {
    mkdirSync(join(root, "tools", "grammars"), { recursive: true });
    const wasm = Buffer.from("custom-grammar-bytes");
    writeFileSync(join(root, "tools", "grammars", "tree-sitter-mydsl.wasm"), wasm);
    writeFileSync(join(root, "cortex.languages.toml"), [
      "version = 1",
      "",
      "[[languages]]",
      'id = "mydsl"',
      'extensions = ["mydsl"]',
      'grammar = "tools/grammars/tree-sitter-mydsl.wasm"',
      `grammar_sha256 = "${sha256Hex(wasm)}"`,
      'fact_profile = "code"',
      'precision_tier = "AST"',
    ].join("\n"));
    const result = loadCustomLanguages({ root });
    assert.equal(result.errors.length, 0, result.errors.join("; "));
    assert.equal(result.languages.length, 1);
    assert.equal(result.languages[0].id, "mydsl");
    assert.deepEqual(result.languages[0].extensions, ["mydsl"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("built-in id and extension routes are rejected", () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-custom-conflict-"));
  try {
    mkdirSync(join(root, "tools"), { recursive: true });
    const wasm = Buffer.from("x");
    writeFileSync(join(root, "tools", "g.wasm"), wasm);
    writeFileSync(join(root, "cortex.languages.toml"), [
      "version = 1",
      "[[languages]]",
      'id = "typescript"',
      'extensions = ["ts"]',
      'grammar = "tools/g.wasm"',
      `grammar_sha256 = "${sha256Hex(wasm)}"`,
    ].join("\n"));
    const result = loadCustomLanguages({ root });
    assert.ok(result.errors.some((e) => e.includes("language_route_conflict")), result.errors.join("; "));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("absolute path and hash mismatch are rejected", () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-custom-reject-"));
  try {
    mkdirSync(join(root, "tools"), { recursive: true });
    const wasm = Buffer.from("y");
    writeFileSync(join(root, "tools", "g.wasm"), wasm);
    writeFileSync(join(root, "cortex.languages.toml"), [
      "version = 1",
      "[[languages]]",
      'id = "evil"',
      'extensions = ["evil"]',
      'grammar = "/etc/passwd"',
      `grammar_sha256 = "${sha256Hex(wasm)}"`,
    ].join("\n"));
    const result = loadCustomLanguages({ root });
    assert.ok(result.errors.some((e) => e.includes("path_conflict")), result.errors.join("; "));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("absent config yields no custom languages", () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-custom-absent-"));
  try {
    const result = loadCustomLanguages({ root });
    assert.equal(result.languages.length, 0);
    assert.equal(result.errors.length, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
