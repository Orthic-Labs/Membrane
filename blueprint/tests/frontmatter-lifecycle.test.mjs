import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.resolve(HERE, "../scripts/blueprint.mjs");

function write(repo, relativePath, body) {
  const target = path.join(repo, relativePath);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, body);
}

function build(repo) {
  const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--no-readme-link"], {
    cwd: repo,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const read = (name) => JSON.parse(fs.readFileSync(path.join(repo, ".agent", name), "utf8"));
  return { map: read("map.json"), claims: read("claims.json"), stale: read("stale.json"), queue: read("queue.json") };
}

function docByPath(map, docPath) {
  return map.nodes.find((node) => node.kind === "doc" && node.path === docPath);
}

function withRepo(name, fn) {
  const repo = path.join(os.tmpdir(), `blueprint-fm-${name}-${process.pid}-${Date.now()}`);
  fs.mkdirSync(repo, { recursive: true });
  try {
    fn(repo);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
}

test("a structured declaration is read from frontmatter and the doc stays live", () => {
  withRepo("accepted", (repo) => {
    write(repo, "README.md", "# Product\n\nThe product is implemented.\n");
    write(
      repo,
      "docs/adr/ADR-001.md",
      [
        "---",
        "blueprint:",
        "  document_id: adr-auth-004",
        "  type: decision",
        "  status: accepted",
        "  effective_from: 2026-06-12",
        "  canonical_for: [authentication, token-storage]",
        "  scope: { deployable_units: [api, desktop], branches: [main] }",
        "---",
        "",
        "# ADR-001",
        "",
        "Token storage is implemented by `src/tokens.ts`.",
        "",
      ].join("\n"),
    );
    write(repo, "src/tokens.ts", "export const tokens = true;\n");

    const { map, claims } = build(repo);
    const adr = docByPath(map, "docs/adr/ADR-001.md");

    assert.equal(adr.lifecycle.status, "current");
    assert.equal(adr.lifecycle.documentId, "adr-auth-004");
    assert.equal(adr.lifecycle.documentType, "decision");
    assert.equal(adr.lifecycle.declaredStatus, "accepted");
    assert.equal(adr.lifecycle.effectiveFrom, "2026-06-12");
    assert.deepEqual(adr.lifecycle.canonicalFor, ["authentication", "token-storage"]);
    assert.deepEqual(adr.lifecycle.scope, {
      deployableUnits: ["api", "desktop"],
      branches: ["main"],
    });
    assert.ok(claims.some((claim) => claim.source === "docs/adr/ADR-001.md"));
  });
});

test("a structured supersedes declaration retires the named document", () => {
  withRepo("supersedes", (repo) => {
    write(repo, "README.md", "# Product\n\nThe product is implemented.\n");
    write(
      repo,
      "docs/adr/ADR-002.md",
      [
        "---",
        "blueprint:",
        "  document_id: adr-auth-002",
        "  type: decision",
        "  status: accepted",
        "---",
        "",
        "# ADR-002",
        "",
        "The legacy gate is implemented by `src/missing.ts`.",
        "",
      ].join("\n"),
    );
    write(
      repo,
      "docs/adr/ADR-004.md",
      [
        "---",
        "blueprint:",
        "  document_id: adr-auth-004",
        "  type: decision",
        "  status: accepted",
        "  effective_from: 2026-06-12",
        "  supersedes: [adr-auth-002]",
        "---",
        "",
        "# ADR-004",
        "",
        "The current gate is implemented by `src/gate.ts`.",
        "",
      ].join("\n"),
    );
    write(repo, "src/gate.ts", "export const gate = true;\n");

    const { map, claims, queue, stale } = build(repo);
    const old = docByPath(map, "docs/adr/ADR-002.md");
    const current = docByPath(map, "docs/adr/ADR-004.md");

    assert.equal(old.lifecycle.status, "superseded");
    assert.equal(old.lifecycle.supersededBy, "docs/adr/ADR-004.md");
    assert.equal(old.lifecycle.supersededOn, "2026-06-12");
    assert.equal(current.lifecycle.status, "current");

    // Retired content must leave every live Blueprint input.
    assert.ok(claims.every((claim) => claim.source !== "docs/adr/ADR-002.md"));
    assert.ok(queue.claims.every((claim) => claim.source !== "docs/adr/ADR-002.md"));
    assert.ok(stale.missingReferences.every((item) => item.source !== "docs/adr/ADR-002.md"));
    assert.ok(claims.some((claim) => claim.source === "docs/adr/ADR-004.md"));

    assert.ok(
      map.edges.some((edge) => edge.type === "supersedes" && edge.from === current.id && edge.to === old.id),
      "expected a supersedes edge from the replacement to the retired document",
    );
  });
});

test("block-style supersedes lists are parsed the same as flow style", () => {
  withRepo("blockstyle", (repo) => {
    write(repo, "README.md", "# Product\n\nThe product is implemented.\n");
    write(
      repo,
      "docs/old.md",
      ["---", "blueprint:", "  document_id: doc-old", "  status: accepted", "---", "", "# Old\n", "Legacy claim.\n", ""].join("\n"),
    );
    write(
      repo,
      "docs/new.md",
      [
        "---",
        "blueprint:",
        "  document_id: doc-new",
        "  status: accepted",
        "  effective_from: 2026-07-01",
        "  supersedes:",
        "    - doc-old",
        "---",
        "",
        "# New\n",
        "Current claim.\n",
        "",
      ].join("\n"),
    );

    const { map } = build(repo);
    assert.equal(docByPath(map, "docs/old.md").lifecycle.status, "superseded");
    assert.equal(docByPath(map, "docs/old.md").lifecycle.supersededBy, "docs/new.md");
    assert.equal(docByPath(map, "docs/new.md").lifecycle.status, "current");
  });
});

test("rejected and draft declarations are excluded from live claims", () => {
  withRepo("noncurrent", (repo) => {
    write(repo, "README.md", "# Product\n\nThe product is implemented.\n");
    write(
      repo,
      "docs/rejected.md",
      ["---", "blueprint:", "  document_id: doc-rejected", "  status: rejected", "---", "", "# Rejected\n", "Rejected claim.\n", ""].join("\n"),
    );
    write(
      repo,
      "docs/draft.md",
      ["---", "blueprint:", "  document_id: doc-draft", "  status: draft", "---", "", "# Draft\n", "Draft claim.\n", ""].join("\n"),
    );

    const { map, claims } = build(repo);
    assert.equal(docByPath(map, "docs/rejected.md").lifecycle.status, "rejected");
    assert.equal(docByPath(map, "docs/draft.md").lifecycle.status, "draft");
    assert.ok(claims.every((claim) => claim.source !== "docs/rejected.md"));
    assert.ok(claims.every((claim) => claim.source !== "docs/draft.md"));
  });
});

test("a document without frontmatter is unaffected by the lifecycle resolver", () => {
  withRepo("plain", (repo) => {
    write(repo, "README.md", "# Product\n\nThe product is implemented.\n");

    const { map } = build(repo);
    const readme = docByPath(map, "README.md");
    assert.equal(readme.lifecycle.status, "current");
    // No frontmatter means no governance keys are added at all, so documents
    // that declare nothing keep byte-identical artifacts.
    assert.equal(readme.lifecycle.documentId, undefined);
    assert.equal(readme.lifecycle.documentType, undefined);
    assert.equal(readme.lifecycle.declaredStatus, undefined);
  });
});
