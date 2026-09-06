// D51 / BPT-057: poisoned manifests and archive traversal. A poisoned plugin
// or grammar manifest must be refused BEFORE it can influence indexing or
// escape the repository root, and a secret in hostile content must never
// survive into egress text.
//
// Every assertion here drives production code. An earlier revision of this
// file defined its own `validateGrammarManifest`, `refuseTraversal` and
// `scanForSecret` closures inline and asserted against those, which proved
// only that the test's own copy of each rule worked — not that Blueprint
// refuses anything.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildHostileRepo } from "../../fixtures/security/build-hostile-repo.mjs";
import { admitPluginManifest, admitRepositoryPlugins, discoverPluginManifests, PLUGIN_MANIFEST_DIR } from "../../src/providers/plugin-loader.mjs";
import { validateGrammarCatalog } from "../../src/graph/treesitter-provider.mjs";
import { redactForEgress } from "../../src/lib/redaction.mjs";

const LICENSE = "SEE LICENSE IN LICENSE";

function repoWithPlugins(manifests, files = {}) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-poison-"));
  mkdirSync(join(root, PLUGIN_MANIFEST_DIR), { recursive: true });
  for (const [name, body] of Object.entries(manifests)) {
    writeFileSync(join(root, PLUGIN_MANIFEST_DIR, name), typeof body === "string" ? body : JSON.stringify(body, null, 2));
  }
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, path, ".."), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}

test("a poisoned grammar catalog entry is refused by the production validator", () => {
  // The real catalog validator is what `catalog()` runs before any grammar is
  // loaded, so a traversal grammar file never reaches the WASM loader.
  const manifest = { grammars: [{ file: "tree-sitter-python.wasm" }] };
  assert.equal(validateGrammarCatalog({ grammars: [{ language: "python", grammarFile: "tree-sitter-python.wasm", extensions: ["py"] }] }, manifest).ok, true);
  for (const grammarFile of ["../../../etc/tree-sitter-evil.wasm", "/abs/tree-sitter-evil.wasm", "tree-sitter-python.wasm.exe"]) {
    const result = validateGrammarCatalog({ grammars: [{ language: "python", grammarFile, extensions: ["py"] }] }, manifest);
    assert.equal(result.ok, false, `poisoned grammar file accepted: ${grammarFile}`);
    assert.match(result.error, /grammar_file|manifest/);
  }
  // A catalog entry naming a grammar the pinned manifest does not list is
  // refused even when the filename itself looks well-formed.
  const unpinned = validateGrammarCatalog({ grammars: [{ language: "python", grammarFile: "tree-sitter-evil.wasm", extensions: ["py"] }] }, manifest);
  assert.equal(unpinned.ok, false);
  assert.match(unpinned.error, /manifest/);
});

test("plugin manifests that escape the repository are refused before any read", () => {
  const root = repoWithPlugins({});
  try {
    for (const path of ["../outside.json", "a/../../escape.json", "/etc/passwd.json"]) {
      const outcome = admitPluginManifest(root, path);
      assert.equal(outcome.disposition, "refused", `traversal manifest path accepted: ${path}`);
      assert.equal(outcome.code, "plugin_manifest_path_escapes_repository");
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a malformed or unknown-type plugin manifest is refused, never skipped", () => {
  const root = repoWithPlugins({
    "broken.json": "{ this is not json",
    "wrongtype.json": { id: "x", version: "1.0.0", type: "arbitrary-code", license: LICENSE, integrity: `sha256:${"0".repeat(64)}`, entry: "plugins/x.mjs" },
  });
  try {
    const receipt = admitRepositoryPlugins(root);
    assert.equal(receipt.considered, 2);
    assert.equal(receipt.admitted.length, 0);
    // "No silent disappearance": both appear, each with the gate that refused it.
    assert.deepEqual(
      receipt.refused.map((row) => row.code).sort(),
      ["plugin_manifest_malformed", "plugin_type_unknown"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an untrusted publisher is refused even when the artifact checksum matches", () => {
  const entry = "export default {};\n";
  const root = repoWithPlugins({
    "third-party.json": {
      id: "third.party",
      version: "1.0.0",
      type: "language-table",
      license: LICENSE,
      publisher: "unknown-publisher",
      integrity: `sha256:${createHash("sha256").update(entry).digest("hex")}`,
      entry: "plugins/third.mjs",
      capabilities: [],
      permissions: {},
    },
  }, { "plugins/third.mjs": entry });
  try {
    const outcome = admitPluginManifest(root, join(PLUGIN_MANIFEST_DIR, "third-party.json"), {
      allowedLicenses: [LICENSE],
      trustedPublishers: ["orthic-labs"],
    });
    assert.equal(outcome.disposition, "refused");
    assert.equal(outcome.code, "plugin_publisher_untrusted");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("discovery is itself confined: a plugin directory outside the repository yields nothing", () => {
  const root = repoWithPlugins({});
  try {
    assert.deepEqual(discoverPluginManifests(root, { dir: "../elsewhere" }), []);
    assert.deepEqual(discoverPluginManifests(root, { dir: "/etc" }), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("secrets in hostile repository content never survive production egress redaction", () => {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-poison-repo-"));
  buildHostileRepo(repo);
  try {
    const blob = "DB_PASSWORD=supersecret-local-value GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd";
    const redacted = redactForEgress(blob);
    assert.ok(!redacted.includes("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd"), "token value is redacted");
    assert.ok(redacted.includes("[REDACTED]"));
    // Key-named fields are redacted structurally, not merely pattern-matched.
    assert.deepEqual(redactForEgress({ password: "supersecret-local-value", note: "fine" }), {
      password: "[REDACTED]",
      note: "fine",
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
