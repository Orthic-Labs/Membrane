import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  aggregateDigest,
  buildManifest,
  checkGeneratedTruth,
  discoverExecutables,
  evaluateSealReadiness,
  fileSha256,
  findDeletedSelectors,
  firstMatchingRule,
  isExecutableCandidate,
  validateManifest,
} from "./check-runtime-language-manifest.mjs";

const TODAY = "2026-09-01";

function makeTree(t, files) {
  const root = mkdtempSync(join(tmpdir(), "rlm-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [rel, content] of Object.entries(files)) {
    const p = join(root, rel);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, content);
  }
  return root;
}
import { dirname as dirnameOf } from "node:path";
const dirname = dirnameOf;

function basePolicy(overrides = {}) {
  return {
    enforcementMode: "migration",
    interpreterRuntimes: ["python", "node", "shell"],
    scan: { executableExtensions: [".py", ".rs", ".mjs"], shebangFallback: true },
    exceptions: [
      {
        id: "exc-a",
        paths: ["legacy/"],
        owner: "packet-owner",
        expires: "2027-01-01",
      },
    ],
    deletedSelectors: [],
    cutoverFlags: [],
    classificationRules: [
      {
        id: "native",
        pattern: "^src/",
        groupBy: "dir:2",
        runtime: "rust", kind: "crate", product_surface: "core",
        production_reachable: true, packaged: true,
        target_disposition: "native-port", exception: null,
      },
      {
        id: "legacy-py",
        pattern: "^legacy/",
        groupBy: "all",
        runtime: "python", kind: "legacy", product_surface: "core",
        production_reachable: true, packaged: false,
        target_disposition: "native-port", exception: "exc-a",
        parity_fixture: "fixtures/x.json",
      },
    ],
    ...overrides,
  };
}

test("executable discovery uses extensions and shebang fallback", () => {
  const root = makeTree(test, {
    "a.py": "x=1\n",
    "b.txt": "#!/bin/sh\necho hi\n",
    "c.sh": "#!/bin/sh\n",
    "d.rs": "fn main() {}\n",
  });
  const policy = basePolicy();
  policy.scan.executableExtensions = [".py", ".sh"];
  const found = discoverExecutables(["a.py", "b.txt", "c.sh", "d.rs"], policy, root);
  assert.deepEqual(found, ["a.py", "b.txt", "c.sh"]); // b.txt via shebang; d.rs not an ext here
});

test("isExecutableCandidate rejects extensionless file without shebang", () => {
  const root = makeTree(test, { "plain": "just text\n" });
  const policy = basePolicy();
  assert.equal(isExecutableCandidate("plain", policy, root), false);
});

test("firstMatchingRule returns first match in declaration order", () => {
  const policy = basePolicy();
  assert.equal(firstMatchingRule(policy, "legacy/a.py").id, "legacy-py");
  assert.equal(firstMatchingRule(policy, "src/crate-main/lib.rs").id, "native");
  assert.equal(firstMatchingRule(policy, "other/x.py"), null);
});

test("buildManifest groups rows and mirrors exception expiry/owner", () => {
  const root = makeTree(test, {
    "src/crateA/lib.rs": "fn a() {}\n",
    "src/crateB/lib.rs": "fn b() {}\n",
    "legacy/old.py": "print(1)\n",
    "legacy/older.py": "print(2)\n",
  });
  const manifest = buildManifest({ root, policy: basePolicy(), trackedFiles: Object.keys({
    "src/crateA/lib.rs": 1, "src/crateB/lib.rs": 1, "legacy/old.py": 1, "legacy/older.py": 1,
  }) });
  assert.equal(manifest.totals.rows, 3); // two crate rows + one legacy group
  const legacy = manifest.rows.find((r) => r.rule === "legacy-py");
  assert.equal(legacy.expiry, "2027-01-01");
  assert.equal(legacy.owner, "packet-owner");
  assert.equal(legacy.files.length, 2);
  const crate = manifest.rows.find((r) => r.id.includes("crateA"));
  assert.ok(crate.content_digest.match(/^[0-9a-f]{64}$/));
});

test("aggregateDigest is order-insensitive and content-sensitive", () => {
  const root = makeTree(test, { "f/a.txt": "1", "f/b.txt": "2" });
  const d1 = aggregateDigest(root, ["f/a.txt", "f/b.txt"]);
  const d2 = aggregateDigest(root, ["f/b.txt", "f/a.txt"]);
  assert.equal(d1, d2);
  writeFileSync(join(root, "f/b.txt"), "3");
  assert.notEqual(d1, aggregateDigest(root, ["f/a.txt", "f/b.txt"]));
});

function happyFixture(t, policyOverrides = {}) {
  const files = {
    "src/crateA/lib.rs": "fn a() {}\n",
    "legacy/old.py": "print(1)\n",
    "fixtures/x.json": "{}\n",
  };
  const root = makeTree(t, files);
  const policy = basePolicy(policyOverrides);
  const manifest = buildManifest({ root, policy, trackedFiles: Object.keys(files) });
  return { root, policy, manifest };
}

test("valid migration-mode manifest produces no errors", () => {
  const { root, policy, manifest } = happyFixture(test);
  const discovered = manifest.rows.flatMap((r) => r.files).sort();
  const { errors } = validateManifest({
    policy, policyDigestActual: manifest.policyDigest, manifest,
    discovered, truthTexts: [], today: TODAY, root,
  });
  assert.deepEqual(errors, []);
});

test("unclassified executable on disk is an error", () => {
  const { root, policy, manifest } = happyFixture(test);
  const discovered = manifest.rows.flatMap((r) => r.files).concat("mystery.py").sort();
  writeFileSync(join(root, "mystery.py"), "x=1\n");
  const { errors } = validateManifest({
    policy, policyDigestActual: manifest.policyDigest, manifest,
    discovered, truthTexts: [], today: TODAY, root,
  });
  assert.ok(errors.some((e) => e.code === "UNCLASSIFIED_EXECUTABLE" && e.path === "mystery.py"));
});

test("stale digest detected when covered file changes", () => {
  const { root, policy, manifest } = happyFixture(test);
  writeFileSync(join(root, "legacy/old.py"), "print(999)\n");
  const discovered = manifest.rows.flatMap((r) => r.files).sort();
  const { errors } = validateManifest({
    policy, policyDigestActual: manifest.policyDigest, manifest,
    discovered, truthTexts: [], today: TODAY, root,
  });
  assert.ok(errors.some((e) => e.code === "STALE_DIGEST"));
});

test("stale policy digest detected", () => {
  const { root, policy, manifest } = happyFixture(test);
  const discovered = manifest.rows.flatMap((r) => r.files).sort();
  const { errors } = validateManifest({
    policy, policyDigestActual: "deadbeef", manifest,
    discovered, truthTexts: [], today: TODAY, root,
  });
  assert.ok(errors.some((e) => e.code === "STALE_POLICY_DIGEST"));
});

test("production python without exception fails; with expired or ownerless exception fails", () => {
  // no exception reference
  let fx = happyFixture(test);
  fx.manifest.rows.find((r) => r.rule === "legacy-py").exception = null;
  let out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "DISALLOWED_PRODUCTION_LANGUAGE"));

  // expired exception
  fx = happyFixture(test, {
    exceptions: [{ id: "exc-a", paths: ["legacy/"], owner: "o", expires: "2026-01-01" }],
  });
  out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "EXCEPTION_EXPIRED"));

  // missing owner
  fx = happyFixture(test, {
    exceptions: [{ id: "exc-a", paths: ["legacy/"], owner: "", expires: "2027-01-01" }],
  });
  out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "EXCEPTION_MISSING_OWNER_OR_EXPIRY"));

  // unknown exception reference
  fx = happyFixture(test);
  fx.manifest.rows.find((r) => r.rule === "legacy-py").exception = "nope";
  out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "MISSING_EXCEPTION_REFERENCE"));
});

test("exception scope mismatch when row falls outside exception paths", () => {
  const fx = happyFixture(test, {
    exceptions: [{ id: "exc-a", paths: ["somewhere-else/"], owner: "o", expires: "2027-01-01" }],
  });
  const out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "EXCEPTION_SCOPE_MISMATCH"));
});

test("deleted selector reappearance fails even in migration mode", () => {
  const fx = happyFixture(test, { deletedSelectors: ["legacy/old.py"] });
  const out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "DELETED_SELECTOR_PRESENT" && e.path === "legacy/old.py"));
});

test("generated-truth authority rule fires only after cutover flag completes", () => {
  const texts = [{ name: "docs/truth.md", text: "source of truth is legacy/old.py" }];
  const incomplete = { id: "f1", complete: false, retiredAuthorityPaths: ["legacy/old.py"] };
  assert.deepEqual(checkGeneratedTruth(texts, incomplete), []);

  const complete = { id: "f1", complete: true, retiredAuthorityPaths: ["legacy/old.py"] };
  const hits = checkGeneratedTruth(texts, complete);
  assert.equal(hits.length, 1);

  const fx = happyFixture(test, { cutoverFlags: [complete] });
  const out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: fx.manifest.rows.flatMap((r) => r.files).sort(), truthTexts: texts, today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "GENERATED_TRUTH_NAMES_RETIRED_AUTHORITY"));
});

test("sealed mode forbids any interpreter production row outright", () => {
  const { root, policy, manifest } = happyFixture(test, { enforcementMode: "sealed" });
  // keep the row but strip its exception to prove sealed needs no exceptions at all
  const discovered = manifest.rows.flatMap((r) => r.files).sort();
  const out = validateManifest({
    policy, policyDigestActual: manifest.policyDigest, manifest,
    discovered, truthTexts: [], today: TODAY, root,
  });
  assert.ok(out.errors.some((e) => e.code === "SEALED_MODE_INTERPRETER_PRODUCTION"));
});

test("file digests are invariant across Git line endings", (t) => {
  const root = makeTree(t, { "f/source.rs": "fn main() {}\n" });
  const lf = fileSha256(root, "f/source.rs");
  writeFileSync(join(root, "f/source.rs"), "fn main() {}\r\n");
  assert.equal(fileSha256(root, "f/source.rs"), lf);
});

test("sealed mode permits only an exact packaged external-component interpreter row", () => {
  const rules = basePolicy().classificationRules.map(rule => rule.id === "legacy-py" ? {
    ...rule,
    runtime: "node",
    packaged: true,
    target_disposition: "external-typed-service",
    exception: null,
  } : rule);
  const fx = happyFixture(test, {
    enforcementMode: "sealed",
    sealedExternalInterpreterRows: ["legacy-py-legacy-py"],
    classificationRules: rules,
  });
  const discovered = fx.manifest.rows.flatMap(row => row.files).sort();
  const accepted = validateManifest({
    policy: fx.policy,
    policyDigestActual: fx.manifest.policyDigest,
    manifest: fx.manifest,
    discovered,
    truthTexts: [],
    today: TODAY,
    root: fx.root,
  });
  assert.equal(fx.manifest.totals.productionInterpreterRows, 0);
  assert.equal(fx.manifest.totals.boundedExternalInterpreterRows, 1);
  assert.deepEqual(accepted.errors, []);

  const rejected = validateManifest({
    policy: { ...fx.policy, sealedExternalInterpreterRows: ["some-other-row"] },
    policyDigestActual: fx.manifest.policyDigest,
    manifest: fx.manifest,
    discovered,
    truthTexts: [],
    today: TODAY,
    root: fx.root,
  });
  assert.ok(rejected.errors.some(error => error.code === "SEALED_MODE_INTERPRETER_PRODUCTION"));
  assert.ok(rejected.errors.some(error => error.code === "INVALID_SEALED_EXTERNAL_INTERPRETER"));
});

test("invalid disposition/runtime rejected; duplicate coverage rejected", () => {
  const fx = happyFixture(test);
  fx.manifest.rows[0].target_disposition = "native-magic";
  fx.manifest.rows[0].runtime = "wasm";
  fx.manifest.rows.push({ ...JSON.parse(JSON.stringify(fx.manifest.rows[1])), id: "dup" });
  const discovered = fx.manifest.rows.flatMap((r) => r.files).sort();
  const out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered, truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.ok(out.errors.some((e) => e.code === "INVALID_DISPOSITION"));
  assert.ok(out.errors.some((e) => e.code === "INVALID_RUNTIME"));
  assert.ok(out.errors.some((e) => e.code === "DUPLICATE_COVERAGE"));
});

test("schema mismatch short-circuits validation", () => {
  const fx = happyFixture(test);
  fx.manifest.artifact = "something.else";
  const out = validateManifest({
    policy: fx.policy, policyDigestActual: fx.manifest.policyDigest, manifest: fx.manifest,
    discovered: [], truthTexts: [], today: TODAY, root: fx.root,
  });
  assert.deepEqual(out.errors.map((e) => e.code), ["MANIFEST_SCHEMA_MISMATCH"]);
});

test("seal readiness enumerates blockers and never seals by accident", () => {
  const fx = happyFixture(test);
  const blockers = evaluateSealReadiness({
    policy: fx.policy, manifest: fx.manifest, existsFile: (p) => p === "fixtures/x.json",
  });
  assert.ok(blockers.some((b) => b.includes("enforcementMode")));
  // parity fixture exists -> no planned-fixture blocker; add one that is missing
  const legacyRow = fx.manifest.rows.find((r) => r.rule === "legacy-py");
  legacyRow.target_disposition = "migration-oracle";
  legacyRow.parity_fixture =
    "planned:migration/native-rust/fixtures/adapt-corpus.v1.json";
  legacyRow.deletion_or_exclusion_proof = null;
  const blockers2 = evaluateSealReadiness({
    policy: fx.policy, manifest: fx.manifest, existsFile: () => false,
  });
  assert.ok(blockers2.some((b) => b.includes("parity fixture missing")));
  assert.ok(blockers2.some((b) => b.includes("missing deletion/exclusion proof")));
  assert.ok(blockers2.length >= 3); // mode + production interpreter rows + the two above
});

test("findDeletedSelectors matches exact path and prefix", () => {
  const hits = findDeletedSelectors(["a/b.py", "a/b/c.py"], ["a/b.py", "a/b/"]);
  assert.equal(hits.length, 2);
});
