// tests/standalone-completion.test.mjs — Blueprint standalone e2e-finish regression suite
//
// Covers the requirements from the `minimax/blueprint-e2e-finish` branch:
//   1. No START-HERE write or entrypoint anywhere in the artifact tree.
//   2. map.json, .agent/manifest.json, generated-document headers, graph manifest,
//      and sourceSignature identify the same generation.
//   3. Generated docs are excluded from the next build's discovery and signature.
//   4. Real non-empty ContextCandidateSet on a non-trivial task with repo evidence.
//   5. Same-slug file paths get stable unique IDs (no duplicate node IDs).
//   6. Stale manifest -> rebuild -> ready state.
//   7. Unchanged rebuild is byte-identical at every artifact.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { createXXHash128 } from "hash-wasm";

const xxhasher = await createXXHash128();

function xxh3Hex(value) {
  xxhasher.init();
  xxhasher.update(value);
  return xxhasher.digest("hex");
}
import { fileURLToPath } from "node:url";
import { readEnvelope, mutateManifest } from "./_store-helpers.mjs";
import { readGeneration } from "../src/graph/static-provider.mjs";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const BLUEPRINT = join(HERE, "..");
const CLI = join(BLUEPRINT, "scripts/blueprint.mjs");
const FIXTURE = join(BLUEPRINT, "evals/fixture-repos/typescript-commerce");


function makeRepo() {
  const root = join(tmpdir(), `blueprint-e2e-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(root, { recursive: true });
  return root;
}

// Seeds a clean repo (no hand-written docs/ARCHITECTURE.md to trip
// docs_conflict on case-insensitive filesystems). Used for tests that
// require docs/product.md and docs/architecture.md to be generated.
function makeCleanRepo() {
  const root = makeRepo();
  mkdirSync(join(root, "src"), { recursive: true });
  mkdirSync(join(root, "resources"), { recursive: true });
  mkdirSync(join(root, "test"), { recursive: true });
  writeFileSync(join(root, "src/handler.ts"),
    "export function productHandler(input: string): string { return input; }\n" +
    "export function routeProduct(input: string): string { return productHandler(input); }\n" +
    "export function saveProduct(input: string): void { productHandler(input); }\n");
  writeFileSync(join(root, "src/util.ts"), "export const util = () => 1;\n");
  writeFileSync(join(root, "resources/policy.json"), '{"tier":"gold"}\n');
  writeFileSync(join(root, "tsconfig.json"), '{"compilerOptions":{"strict":true}}\n');
  writeFileSync(join(root, "README.md"), "# Sample\n\nSample repo for blueprint standalone completion.\n");
  writeFileSync(join(root, "AGENTS.md"), "# Agents\n\n- The product handles transcription.\n- Local-first is implemented.\n");
  return root;
}

function runCli(repo, args) {
  const result = spawnSync("node", [CLI, ...args], {
    cwd: repo,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.status !== 0 && !args.includes("--check") && !args.includes("doctor")) {
    throw new Error(`blueprint ${args.join(" ")} exited ${result.status}: ${result.stderr}`);
  }
  return result.stdout;
}

// 1) No START-HERE write or entrypoint anywhere in the artifact tree.
test("standalone: no START-HERE.md is written; map entrypoint is .agent/manifest.json", () => {
  const repo = makeRepo();
  try {
    runCli(repo, ["build", "--out", ".agent", "--check"]);
    assert.equal(
      existsSync(join(repo, ".agent/START-HERE.md")),
      false,
      ".agent/START-HERE.md must not be written (retired)",
    );
    const map = JSON.parse(readFileSync(join(repo, ".agent/map.json"), "utf8"));
    assert.equal(map.entrypoint, ".agent/manifest.json", "map.entrypoint must point at portable manifest");
    const manifest = JSON.parse(readFileSync(join(repo, ".agent/manifest.json"), "utf8"));
    assert.equal(manifest.entrypoint, ".agent/", "manifest.entrypoint must point at .agent/");
    const skillText = readFileSync(join(BLUEPRINT, "skills/blueprint/SKILL.md"), "utf8");
    const matches = [...skillText.matchAll(/START-HERE\.md/g)];
    assert.ok(matches.length <= 2, `SKILL.md mentions START-HERE ${matches.length} times; retirement notes only`);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// 2) Every artifact agrees on the generation identity.
test("standalone: every artifact agrees on generation identity", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    const map = JSON.parse(readFileSync(join(repo, ".agent/map.json"), "utf8"));
    const manifest = JSON.parse(readFileSync(join(repo, ".agent/manifest.json"), "utf8"));
    const graphManifest = readEnvelope(repo);
    const product = readFileSync(join(repo, "docs/product.md"), "utf8");
    const arch = readFileSync(join(repo, "docs/architecture.md"), "utf8");
    const index = JSON.parse(readFileSync(join(repo, ".agent/index.json"), "utf8"));
    const graphId = graphManifest.generationId;
    assert.equal(manifest.generation.id, graphId, "manifest.generation.id must equal graph generationId");
    assert.equal(manifest.generation.revision, graphId, "manifest.generation.revision must equal graph generationId");
    const productGen = product.match(/gen:([a-f0-9]+)/);
    const archGen = arch.match(/gen:([a-f0-9]+)/);
    assert.ok(productGen, "product.md missing gen: marker");
    assert.ok(archGen, "architecture.md missing gen: marker");
    assert.equal(productGen[1], archGen[1], "product.md and architecture.md must share the same gen: id");
    assert.equal(
      manifest.generation.contentHash,
      graphManifest.repo?.sourceHash ?? null,
      "manifest contentHash must equal graph sourceHash",
    );
    assert.equal(typeof index.sourceSignature, "string", "index.sourceSignature must be a string");
    assert.ok(index.sourceSignature.length > 0, "index.sourceSignature must not be empty");
    assert.equal(manifest.sourceSignature, index.sourceSignature, "manifest.sourceSignature must equal index.sourceSignature");
    assert.equal(map.repo, manifest.repo.rootName, "map.repo and manifest.repo.rootName must agree");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// 3) Generated docs excluded from next build's discovery + signature.
test("standalone: generated docs do not perturb the next build's signature", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    const idx1 = JSON.parse(readFileSync(join(repo, ".agent/index.json"), "utf8"));
    const product1 = readFileSync(join(repo, "docs/product.md"), "utf8");
    const arch1 = readFileSync(join(repo, "docs/architecture.md"), "utf8");
    runCli(repo, ["build", "--out", ".agent"]);
    const idx2 = JSON.parse(readFileSync(join(repo, ".agent/index.json"), "utf8"));
    const product2 = readFileSync(join(repo, "docs/product.md"), "utf8");
    const arch2 = readFileSync(join(repo, "docs/architecture.md"), "utf8");
    assert.equal(idx1.sourceSignature, idx2.sourceSignature, "sourceSignature must not change between unchanged rebuilds");
    assert.equal(product1, product2, "docs/product.md must be byte-identical across unchanged rebuilds");
    assert.equal(arch1, arch2, "docs/architecture.md must be byte-identical across unchanged rebuilds");
    const generated = idx1.files.map((f) => f.path);
    assert.ok(!generated.includes("docs/product.md"), "docs/product.md must not appear in index.files");
    assert.ok(!generated.includes("docs/architecture.md"), "docs/architecture.md must not appear in index.files");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// 4) Real non-empty ContextCandidateSet with repo evidence.
test("standalone: graph candidates emits schema-valid ContextCandidateSet with real evidence", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    const out = runCli(repo, ["graph", "candidates", "--task", "find product handler"]);
    const set = JSON.parse(out);
    assert.equal(set.schemaVersion, 1, "ContextCandidateSet schemaVersion must be 1");
    assert.equal(set.mode, "survey", "default mode is survey");
    assert.ok(Array.isArray(set.candidates), "candidates must be an array");
    assert.ok(set.candidates.length > 0, "candidates must be non-empty for a real repo");
    const required = [
      "id", "layer", "sourceKind", "sourceRef", "sourceHash", "trustClass",
      "instructionPolicy", "providerScore", "estimatedTokens", "protected", "exact",
      "recoverable", "resolver", "text",
    ];
    for (const c of set.candidates) {
      for (const field of required) {
        assert.ok(field in c, `candidate missing required field: ${field}`);
      }
      assert.match(c.sourceHash, /^xxh128:[a-f0-9]{32}$/, `sourceHash must be xxh128:<hex>: ${c.id}`);
      assert.match(c.sourceRef, /:\d+-\d+$/, `sourceRef must include path:startLine-endLine: ${c.id}`);
    }
    const manifest = JSON.parse(readFileSync(join(repo, ".agent/manifest.json"), "utf8"));
    assert.equal(set.freshness.revision, manifest.generation.id, "candidate set freshness.revision must match manifest.generation.id");
    assert.equal(set.provider, manifest.generation.toolVersions.blueprint, "candidate set provider must match manifest");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("standalone: graph candidates never rebuilds a stale legacy manifest", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    mutateManifest(repo, (manifest) => { delete manifest.fileLimit; });
    writeFileSync(join(repo, "src/handler.ts"), "export function changedHandler() { return 'changed'; }\n");

    const result = spawnSync(process.execPath, [CLI, "graph", "candidates", "--task", "handler"], {
      cwd: repo,
      encoding: "utf8",
      timeout: 2000,
    });
    assert.notEqual(result.status, 0, "query must fail fast instead of rebuilding");
    assert.equal(result.signal, null, "query must return before timeout");
    assert.match(result.stderr, /graph_rebuild_required/);
    const after = readEnvelope(repo);
    assert.equal("fileLimit" in after, false, "query must not mutate the legacy manifest");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("federated graph candidates trust only the exact freshness generation", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    const manifest = readEnvelope(repo);
    writeFileSync(join(repo, "src/handler.ts"), "export function changedHandler() { return 'changed'; }\n");

    const trusted = spawnSync(process.execPath, [
      CLI,
      "graph", "candidates",
      "--task", "handler",
      "--expected-generation", manifest.generationId,
    ], { cwd: repo, encoding: "utf8", timeout: 2000 });
    assert.equal(trusted.status, 0, trusted.stderr || trusted.stdout);
    const candidateSet = JSON.parse(trusted.stdout);
    assert.equal(candidateSet.freshness.revision, manifest.generationId);

    const rejected = spawnSync(process.execPath, [
      CLI,
      "graph", "candidates",
      "--task", "handler",
      "--expected-generation", `xxh128:${"0".repeat(32)}`,
    ], { cwd: repo, encoding: "utf8", timeout: 2000 });
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /graph_generation_changed/);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// The graph is a local, gitignored index — not a committed artifact — so a
// dirty tree is a normal state to build from, and the build must include the
// working-tree content rather than refusing. The old `graph_build_deferred_dirty`
// refusal existed only because graph.json was tracked in git, which made the
// graph structurally stale during exactly the period a developer needs it fresh.
test("build indexes a dirty working tree instead of refusing", () => {
  const repo = makeCleanRepo();
  try {
    assert.equal(spawnSync("git", ["init", "--quiet"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["config", "user.email", "test@example.invalid"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["config", "user.name", "Test"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["add", "-A"], { cwd: repo }).status, 0);
    assert.equal(spawnSync("git", ["commit", "--quiet", "-m", "fixture"], { cwd: repo }).status, 0);
    runCli(repo, ["build", "--out", ".agent"]);
    writeFileSync(join(repo, "src/handler.ts"), "export function dirtyHandler() { return 'dirty'; }\n");

    const rebuilt = spawnSync(process.execPath, [CLI, "build", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(rebuilt.status, 0, rebuilt.stderr || rebuilt.stdout);
    assert.doesNotMatch(rebuilt.stderr ?? "", /graph_build_deferred_dirty/);
    const generation = readGeneration(repo, ".agent");
    assert.ok(
      generation.nodes.some((node) => node.id === "symbol:src/handler.ts::dirtyHandler"),
      "the uncommitted symbol must be in the graph",
    );
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// 5) Same-slug file paths get stable unique IDs.
test("standalone: same-slug paths receive collision-safe IDs", () => {
  const repo = makeRepo();
  try {
    mkdirSync(join(repo, "src"), { recursive: true });
    writeFileSync(join(repo, "src/foo.md"), "# foo\n\n- implemented\n");
    writeFileSync(join(repo, "src_foo.md"), "# foo_underscore\n\n- implemented\n");
    runCli(repo, ["build", "--out", ".agent"]);
    const map = JSON.parse(readFileSync(join(repo, ".agent/map.json"), "utf8"));
    const ids = map.nodes.map((n) => n.id);
    const unique = new Set(ids);
    const dups = ids.filter((id, i) => ids.indexOf(id) !== i);
    assert.equal(unique.size, ids.length, `node ids must be unique; duplicates: ${dups.join(", ")}`);
    const docs = map.nodes.filter((n) => n.kind === "doc").map((n) => n.path);
    assert.ok(docs.includes("src/foo.md"), "src/foo.md must be a doc node");
    assert.ok(docs.includes("src_foo.md"), "src_foo.md must be a doc node");
    const docIds = map.nodes.filter((n) => n.kind === "doc").map((n) => n.id);
    assert.equal(new Set(docIds).size, docIds.length, "doc ids must be unique across same-slug paths");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// 6) Stale -> rebuild -> ready.
test("standalone: stale graph flips to fresh after rebuild", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    const doctor1 = JSON.parse(runCli(repo, ["doctor", "--json"]));
    assert.ok(["ready", "degraded"].includes(doctor1.state), `expected ready/degraded, got ${doctor1.state}: ${JSON.stringify(doctor1.reasons)}`);
    const srcFile = join(repo, "src/handler.ts");
    const original = readFileSync(srcFile, "utf8");
    writeFileSync(srcFile, original + "\n// stale probe\n");
    const doctor2 = JSON.parse(runCli(repo, ["doctor", "--json"]));
    assert.equal(doctor2.state, "stale", "doctor must report stale after source mutation");
    assert.ok(
      doctor2.reasons.some((r) => r.code === "stale_graph"),
      "doctor must surface a stale_graph reason",
    );
    runCli(repo, ["build", "--out", ".agent"]);
    const doctor3 = JSON.parse(runCli(repo, ["doctor", "--json"]));
    assert.ok(["ready", "degraded"].includes(doctor3.state), `rebuilt doctor must be ready/degraded, got ${doctor3.state}`);
    writeFileSync(srcFile, original);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// 7) Unchanged rebuild byte-identical at every artifact.
test("standalone: unchanged rebuild is byte-identical at every artifact", () => {
  const repo = makeCleanRepo();
  try {
    runCli(repo, ["build", "--out", ".agent"]);
    const artifactPaths = [
      ".agent/map.json",
      ".agent/claims.json",
      ".agent/stale.json",
      ".agent/index.json",
      ".agent/queue.json",
      ".agent/flows.json",
      ".agent/manifest.json",
      "docs/product.md",
      "docs/architecture.md",
    ];
    const generationId = () => readEnvelope(repo).generationId;
    const hashes1 = Object.fromEntries(
      artifactPaths.map((rel) => [rel, xxh3Hex(readFileSync(join(repo, rel), "utf8"))]),
    );
    const generation1 = generationId();
    runCli(repo, ["build", "--out", ".agent"]);
    const hashes2 = Object.fromEntries(
      artifactPaths.map((rel) => [rel, xxh3Hex(readFileSync(join(repo, rel), "utf8"))]),
    );
    for (const rel of artifactPaths) {
      assert.equal(hashes2[rel], hashes1[rel], `${rel} must be byte-identical across unchanged rebuilds`);
    }
    // The graph store is a SQLite file, where byte-identity is not a meaningful
    // property (page layout, freelists and WAL state vary independently of
    // content). The determinism that actually matters is that an unchanged
    // rebuild yields the same generation, so assert that directly.
    assert.equal(generationId(), generation1, "graph store must yield the same generation across unchanged rebuilds");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// Doctor JSON must always be valid JSON and never mix text into stdout.
test("standalone: doctor --json output is parseable JSON even on missing artifacts", () => {
  const repo = makeRepo();
  try {
    const raw = runCli(repo, ["doctor", "--json"]);
    let parsed;
    assert.doesNotThrow(() => { parsed = JSON.parse(raw); }, "doctor --json must emit parseable JSON");
    assert.equal(parsed.schemaVersion, 1);
    assert.ok(["missing", "ready", "stale", "degraded", "broken", "corrupt"].includes(parsed.state), `state must be typed, got ${parsed.state}`);
    assert.ok(Array.isArray(parsed.reasons), "reasons must be an array");
    assert.ok(parsed.reasons.some((r) => r.code === "missing_map"), "missing-map reason must be reported when no build has run");
    for (const [key, value] of Object.entries(parsed.artifacts ?? {})) {
      if (typeof value !== "string") continue;
      assert.ok(!/^[A-Z]:[\\/]/.test(value), `artifact path must not be absolute (${key}: ${value})`);
      assert.ok(!value.startsWith("/"), `artifact path must not be absolute (${key}: ${value})`);
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

// Discovery must skip .agent/, .audit/, caches, worktrees.
test("standalone: hidden directories are excluded from source discovery", () => {
  const repo = makeRepo();
  try {
    mkdirSync(join(repo, ".agent"), { recursive: true });
    writeFileSync(join(repo, ".agent/leaked.md"), "# leaked\n");
    mkdirSync(join(repo, ".cache"), { recursive: true });
    writeFileSync(join(repo, ".cache/leaked.md"), "# leaked\n");
    mkdirSync(join(repo, "node_modules"), { recursive: true });
    writeFileSync(join(repo, "node_modules/leaked.md"), "# leaked\n");
    mkdirSync(join(repo, "src"), { recursive: true });
    writeFileSync(join(repo, "src/legit.ts"), "export const x = 1;\n");
    writeFileSync(join(repo, "README.md"), "# readme\n");
    runCli(repo, ["build", "--out", ".agent"]);
    const idx = JSON.parse(readFileSync(join(repo, ".agent/index.json"), "utf8"));
    const paths = idx.files.map((f) => f.path);
    for (const forbidden of [".agent/leaked.md", ".cache/leaked.md", "node_modules/leaked.md"]) {
      assert.ok(!paths.includes(forbidden), `${forbidden} must be excluded from source discovery`);
    }
    assert.ok(paths.includes("src/legit.ts"), "real source files must be included");
    assert.ok(paths.includes("README.md"), "canonical docs must be included");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
