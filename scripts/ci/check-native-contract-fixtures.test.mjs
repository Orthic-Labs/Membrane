#!/usr/bin/env node
// Tests for scripts/ci/check-native-contract-fixtures.mjs (migration spec N1).
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import {
  FIXTURES_DIR,
  MANIFEST_REL,
  PUBLIC_REGISTRY_DIR,
  aggregateDigest,
  buildManifest,
  canonicalFixtureBytes,
  fixtureSha256,
  sha256,
  validateAgainstSchema,
  validateFixtures,
} from "./check-native-contract-fixtures.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

test("sha256 matches a known vector", () => {
  assert.equal(sha256(Buffer.from("abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
});
test("fixture hashes are invariant across native text line endings", () => {
  assert.equal(
    sha256(canonicalFixtureBytes(Buffer.from("one\r\ntwo\r\n"))),
    sha256(Buffer.from("one\ntwo\n")),
  );
});
test("aggregateDigest is order-insensitive and content-sensitive", () => {
  const a = `${FIXTURES_DIR}/insight-issue-v1.schema.json`;
  const b = `${FIXTURES_DIR}/failure-episode-v1.schema.json`;
  assert.equal(aggregateDigest(REPO, [a, b]), aggregateDigest(REPO, [b, a]));
  assert.notEqual(aggregateDigest(REPO, [a, b]), aggregateDigest(REPO, [a]));
});

test("validateAgainstSchema enforces required, enum, const, pattern, types, additionalProperties", () => {
  const schema = {
    type: "object",
    required: ["id", "kind"],
    additionalProperties: false,
    properties: {
      id: { type: "string", pattern: "^[a-z]+$" },
      kind: { enum: ["a", "b"] },
      tag: { const: "x.v1" },
      count: { type: "integer", minimum: 0 },
      tags: { type: "array", items: { type: "string" } },
      meta: { type: "object", additionalProperties: { type: "string" } },
    },
  };
  const good = validateAgainstSchema({ id: "abc", kind: "a", tag: "x.v1", count: 2, tags: ["q"], meta: { k: "v" } }, schema);
  assert.deepEqual(good, []);

  const bad = validateAgainstSchema({ id: "ABC", kind: "z", tag: "wrong", count: -1, extra: true }, schema);
  const joined = bad.join("\n");
  assert.ok(bad.some((e) => e.includes("pattern")), joined);
  assert.ok(bad.some((e) => e.includes("enum")), joined);
  assert.ok(bad.some((e) => e.includes("const")), joined);
  assert.ok(bad.some((e) => e.includes("minimum")), joined);
  assert.ok(bad.some((e) => e.includes("additional property 'extra'")), joined);

  const missing = validateAgainstSchema({ id: "abc" }, schema);
  assert.ok(missing.some((e) => e.includes("missing required property 'kind'")));
});

function corpusFiles(root) {
  const out = [];
  const stack = [join(root, FIXTURES_DIR)];
  while (stack.length) {
    const d = stack.pop();
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const full = join(d, e.name);
      if (e.isDirectory()) stack.push(full);
      else {
        const rel = full.slice(root.length + 1).split("\\").join("/");
        if (rel.endsWith(".json") || rel.endsWith(".md")) out.push(rel);
      }
    }
  }
  return out.sort();
}

test("buildManifest hashes every fixture file and reproduces aggregateSha256", () => {
  const manifest = buildManifest({ root: REPO });
  assert.equal(
    manifest.contracts.some((contract) => contract.name === "UserActEvidenceV1"),
    false,
    "retired UserActEvidenceV1 must not be regenerated",
  );
  for (const c of manifest.contracts) {
    assert.equal(c.visibility, "internal");
    for (const f of c.fixtureFiles) {
      const actual = fixtureSha256(join(REPO, f.path));
      assert.equal(f.sha256, actual, `${c.name}: ${f.path}`);
    }
  }
  assert.equal(manifest.aggregateSha256, aggregateDigest(REPO, corpusFiles(REPO)));
});

test("golden examples validate against their schemas via the bounded validator", () => {
  const manifest = JSON.parse(readFileSync(join(REPO, MANIFEST_REL), "utf8"));
  for (const c of manifest.contracts) {
    const schemaEntry = c.fixtureFiles.find((f) => f.role === "schema");
    const exampleEntry = c.fixtureFiles.find((f) => f.role === "golden-example");
    if (!exampleEntry) continue;
    const schema = JSON.parse(readFileSync(join(REPO, schemaEntry.path), "utf8"));
    const example = JSON.parse(readFileSync(join(REPO, exampleEntry.path), "utf8"));
    const errs = validateAgainstSchema(example, schema);
    assert.deepEqual(errs, [], `${c.name} golden example violations`);
  }
});

test("validateFixtures accepts the frozen v1 corpus in the real checkout", () => {
  const manifest = JSON.parse(readFileSync(join(REPO, MANIFEST_REL), "utf8"));
  const { errors, warnings } = validateFixtures({ root: REPO, manifest });
  assert.deepEqual(errors, [], JSON.stringify(errors, null, 2));
  assert.deepEqual(warnings, [], JSON.stringify(warnings, null, 2));
});

test("in-place fixture edits are detected; internal contracts stay out of the public registry", () => {
  const root = mkdtempSync(join(tmpdir(), "native-fixtures-test-"));
  try {
    mkdirSync(join(root, PUBLIC_REGISTRY_DIR), { recursive: true });
    writeFileSync(join(root, PUBLIC_REGISTRY_DIR, "scope-grant.v1.schema.json"), "{}\n");
    cpSync(join(REPO, FIXTURES_DIR), join(root, FIXTURES_DIR), { recursive: true });

    // tamper with a hashed fixture AFTER the manifest recorded its digest:
    let manifest = buildManifest({ root });
    const schemaRel = `${FIXTURES_DIR}/transcript-event-v1.schema.json`;
    const orig = readFileSync(join(REPO, schemaRel), "utf8");
    writeFileSync(join(root, schemaRel), orig.replace("TranscriptEventV1", "TranscriptEventV1X"));
    let result = validateFixtures({ root, manifest });
    assert.ok(result.errors.some((e) => e.code === "FIXTURE_HASH_MISMATCH"), JSON.stringify(result.errors));

    // an internal contract leaking into the public registry must be flagged:
    writeFileSync(join(root, PUBLIC_REGISTRY_DIR, "leak.schema.json"), JSON.stringify({ title: "TranscriptEventV1 public clone" }));
    result = validateFixtures({ root, manifest: buildManifest({ root }) });
    assert.ok(result.errors.some((e) => e.code === "INTERNAL_CONTRACT_IN_PUBLIC_REGISTRY"));

    // restoring content and removing the leak returns to clean:
    rmSync(join(root, PUBLIC_REGISTRY_DIR, "leak.schema.json"));
    writeFileSync(join(root, schemaRel), orig);
    result = validateFixtures({ root, manifest: buildManifest({ root }) });
    const structural = result.errors.filter(
      (e) => !["STALE_AGGREGATE_DIGEST"].includes(e.code),
    );
    // synthetic tree lacks sibling fixtures/examples referenced by other contracts
    const onlyMissing = structural.every((e) => e.code === "FIXTURE_MISSING");
    assert.ok(onlyMissing, JSON.stringify(structural));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
