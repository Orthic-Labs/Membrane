// D35: federation security and portable evidence packs.

import assert from "node:assert/strict";
import test from "node:test";

import { composeFederatedSlices, isRepoAllowed } from "../lib/federation/index.mjs";
import { buildEvidencePack, evidencePackToMarkdown } from "../lib/evidence-pack.mjs";
import { redactForEgress } from "../lib/redaction.mjs";

test("federation composes slices without raw-merging", () => {
  const composed = composeFederatedSlices([
    { repoId: "a", generationId: "g1", results: [{ id: "x" }] },
    { repoId: "b", generationId: "g1", results: [{ id: "y" }] },
  ]);
  assert.equal(composed.kind, "federated");
  assert.equal(composed.repos.length, 2);
  assert.equal(composed.results.length, 2);
  for (const repo of composed.repos) assert.equal(repo.generationId, "g1");
});

test("generation ambiguity is rejected", () => {
  assert.throws(
    () => composeFederatedSlices([
      { repoId: "a", generationId: "g1", results: [] },
      { repoId: "a", generationId: "g2", results: [] },
    ]),
    (error) => error.code === "generation_ambiguity",
  );
});

test("incomplete slices are rejected", () => {
  assert.throws(
    () => composeFederatedSlices([{ repoId: "a", results: [] }]),
    (error) => error.code === "slice_incomplete",
  );
});

test("repo access is scoped to allowed ids", () => {
  assert.equal(isRepoAllowed({ repoId: "a" }, ["a", "b"]), true);
  assert.equal(isRepoAllowed({ repoId: "c" }, ["a", "b"]), false);
});

test("evidence packs carry citations, tiers, omissions, and digests", () => {
  const pack = buildEvidencePack({
    repoId: "repo-1",
    generationId: "g1",
    providerTiers: ["AST", "LEXICAL"],
    results: [{ id: "symbol:a", path: "a.ts", span: { startLine: 1, endLine: 2 }, confidenceTier: "EXACT_RESOLUTION", evidence: [{ path: "a.ts", startLine: 1, endLine: 2, contentHash: "h" }] }],
    omissions: [{ reason: "over_budget" }],
    cursor: "next-page",
  });
  assert.equal(pack.schemaVersion, 1);
  assert.ok(pack.packDigest);
  assert.equal(pack.results[0].path, "a.ts");
  assert.equal(pack.omissions[0].reason, "over_budget");
  assert.equal(pack.continuationCursor, "next-page");
});

test("evidence packs render to markdown with citations", () => {
  const pack = buildEvidencePack({ repoId: "r", generationId: "g", results: [{ id: "s", path: "a.ts", span: { startLine: 3, endLine: 4 } }] });
  const md = evidencePackToMarkdown(pack);
  assert.ok(md.includes("a.ts:3-4"));
  assert.ok(md.includes(pack.packDigest));
});

test("evidence packs are redacted on egress", () => {
  const pack = buildEvidencePack({ repoId: "r", generationId: "g", results: [{ id: "s", path: "a.ts", evidence: [{ path: "a.ts", contentHash: "ghp_secretvalue1234567890abcdef" }] }] });
  const json = JSON.stringify(pack);
  assert.ok(!json.includes("ghp_secretvalue"));
});
