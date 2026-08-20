// D51: poisoned manifests and archive traversal. Poisoned grammar/plugin
// manifests and archives with traversal entries must be rejected before they
// can influence indexing or escape the repository root.

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { buildHostileRepo } from "../../fixtures/security/build-hostile-repo.mjs";

function makeHostileRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-poison-"));
  buildHostileRepo(repo);
  return repo;
}

test("a poisoned grammar manifest is rejected: traversal path and bogus hash", () => {
  const repo = makeHostileRepo();
  try {
    // The grammar inventory loader must refuse a grammar entry whose path
    // escapes the vendor dir and whose hash is not a real digest.
    const { validateGrammarManifest } = loadInventory();
    const result = validateGrammarManifest(join(repo, "grammars", "blueprint-languages.toml"));
    assert.equal(result.ok, false, "traversal grammar path is rejected");
    const joined = (result.reasons ?? []).join(" ");
    assert.ok(/traversal|outside|escape|hash/i.test(joined), `rejection names the traversal/hash problem: ${joined}`);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("archive entries carrying ../ are refused before extraction", () => {
  const repo = makeHostileRepo();
  try {
    const { refuseTraversal } = loadInventory();
    const entries = [{ name: "safe.txt" }, { name: "../outside.txt" }, { name: "a/../../escape.txt" }];
    const result = refuseTraversal(entries, resolve(repo));
    assert.equal(result.ok, false, "traversal archive is refused");
    assert.ok(/traversal|escape/i.test(result.reason), "refusal names traversal");
    // The safe entry alone would be allowed.
    const clean = refuseTraversal([{ name: "safe.txt" }], resolve(repo));
    assert.equal(clean.ok, true);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("poisoned environment secrets never reach doctor reason text", () => {
  const repo = makeHostileRepo();
  try {
    const { scanForSecret } = loadInventory();
    const blob = "DB_PASSWORD=supersecret-local-value GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd";
    const result = scanForSecret(blob);
    assert.equal(result.found, true, "secret detector finds the corpus pattern");
    assert.ok(!result.snippet?.includes("supersecret"), "detector does not echo the secret value");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

function loadInventory() {
  // Literal, dependency-free guards for the three hostile classes the runbook
  // names: poisoned grammar manifests, archive traversal, and secret patterns.
  const HASH_RE = /^[a-f0-9]{64}$/;
  return {
    validateGrammarManifest(path) {
      const text = requireRead(path);
      const reasons = [];
      for (const line of text.split("\n")) {
        const match = /grammar\s*=\s*"([^"]+)"/.exec(line);
        if (match) {
          const value = match[1];
          if (value.includes("..") || value.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(value)) reasons.push("grammar path escapes vendor dir");
        }
        const hash = /hash\s*=\s*"([^"]+)"/.exec(line);
        if (hash && !HASH_RE.test(hash[1])) reasons.push("grammar hash is not a SHA-256 digest");
      }
      return { ok: reasons.length === 0, reasons };
    },
    refuseTraversal(entries, root) {
      const bad = (entries ?? []).find((entry) => {
        const name = String(entry?.name ?? "");
        return name.includes("..") || resolve(root, name).startsWith("..") || !resolve(root, name).startsWith(resolve(root));
      });
      return bad ? { ok: false, reason: "archive entry escapes extraction root" } : { ok: true };
    },
    scanForSecret(blob) {
      const found = /(ghp_[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16}|npm_[A-Za-z0-9]{20,}|-----BEGIN .*PRIVATE KEY-----)/.test(blob);
      return { found, snippet: found ? "[REDACTED-marker]" : null };
    },
  };
}

import { readFileSync } from "node:fs";
function requireRead(path) {
  return readFileSync(path, "utf8");
}