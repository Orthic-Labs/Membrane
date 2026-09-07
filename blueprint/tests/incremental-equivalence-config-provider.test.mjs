// BPT-021 closure — CONFIG CHANGE and PROVIDER CHANGE cases.
//
// The nine-case canon ("add/remove/rename/move/config/provider/crash/dirty/
// no-op") had real cold-vs-incremental equivalence proof for six cases
// (resolver-ghost-edge-equivalence.test.mjs, incremental-index.test.mjs,
// freshness-regressions.test.mjs) but only synthetic unit coverage over bare
// `configDigest`/`providerDigest` strings (dependency-dag.test.mjs) for the
// other two. This file closes that gap using the SAME idiom as
// resolver-ghost-edge-equivalence.test.mjs: build an "incremental" repo across
// two builds and a from-scratch "cold" repo with identical final state, then
// assert the node/edge output is equal.
//
// CONFIG: the only real per-repository config Blueprint's production module
// resolver reads live off disk on every build (src/providers/modules/
// javascript.mjs, CONFIG_NAMES = ["tsconfig.json", "jsconfig.json"]) is a
// tsconfig `paths`/`baseUrl` map. Adding one changes which IMPORTS edges
// resolve via `tsconfig_paths`/`tsconfig_base_url` instead of staying
// unresolved. (`configDigest` in dependency-dag.mjs is a separate, unwired
// projection-invalidation input — nothing in the production build path ever
// sets `generation.augmentation.configDigest`, so it cannot be exercised
// end-to-end; see dependency-dag.test.mjs for its existing synthetic coverage.)
//
// PROVIDER: the real provider-identity axis in the production build path is
// the Tree-sitter AST layer toggle (src/graph/provider-identity.mjs
// STATIC_PROVIDER vs TREESITTER_PROVIDER, gated by BLUEPRINT_TREESITTER / the
// `treesitter` build option and driven end-to-end through the CLI's `build`
// command, per freshness-regressions.test.mjs's
// "full production build records lexical and Tree-sitter fact ownership").
// Turning it on between builds is a real provider change: it adds a second
// provider layer and Tree-sitter-sourced facts to the graph.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");

function makeRepo(prefix, files) {
  const root = mkdtempSync(join(tmpdir(), prefix));
  execFileSync("git", ["init", "-q"], { cwd: root });
  for (const [path, content] of Object.entries(files)) {
    const destination = join(root, path);
    mkdirSync(join(destination, ".."), { recursive: true });
    writeFileSync(destination, content);
  }
  execFileSync("git", ["add", "-A"], { cwd: root });
  return root;
}

function edgeProjection(generation) {
  return generation.edges.map(({ id, kind, source, target, resolved, specifier, reason, confidence, confidenceTier }) => ({
    id, kind, source, target, resolved, specifier, reason: reason ?? null, confidence, confidenceTier,
  }));
}

function nodeProjection(generation) {
  return generation.nodes.map(({ id, kind, labels, name, qualifiedName, path }) => ({ id, kind, labels, name, qualifiedName, path }));
}

// ---------------------------------------------------------------------------
// CONFIG CHANGE: adding a tsconfig `paths` alias between builds must resolve
// the same set of IMPORTS edges an equivalent from-scratch build resolves.
// ---------------------------------------------------------------------------

test("config equivalence: adding tsconfig paths resolves the same edges incrementally as cold", () => {
  const initial = {
    "src/util.ts": "export function helper() { return 1; }\n",
    "src/caller.ts": 'import { helper } from "@lib/util";\nexport function call() { return helper(); }\n',
  };
  const tsconfig = JSON.stringify({
    compilerOptions: { baseUrl: ".", paths: { "@lib/*": ["src/*"] } },
  });

  const incrementalRoot = makeRepo("blueprint-config-eq-incremental-", initial);
  const coldRoot = makeRepo("blueprint-config-eq-cold-", { ...initial, "tsconfig.json": tsconfig });
  try {
    // Build 1: no tsconfig yet — the bare `@lib/util` specifier has no
    // relative-import fallback, so the production module resolver (which only
    // materializes an IMPORTS edge once it has a resolved target file, or a
    // provider claim attached to an edge that already exists — see
    // addModuleEvidence in src/providers/build.mjs) records no edge for it at all.
    const before = buildGraphGeneration(incrementalRoot, { outDir: ".agent", persist: true });
    const importEdgeBefore = before.edges.find((e) => e.kind === "IMPORTS" && e.specifier === "@lib/util");
    assert.equal(importEdgeBefore, undefined, "without tsconfig the aliased specifier must not yet appear as an edge");

    // Config change: write tsconfig.json (no source file touched) and rebuild
    // incrementally — this is the real per-repo config the production module
    // resolver reads live off disk every build.
    writeFileSync(join(incrementalRoot, "tsconfig.json"), tsconfig);
    execFileSync("git", ["add", "-A"], { cwd: incrementalRoot });
    const incremental = buildGraphGeneration(incrementalRoot, { outDir: ".agent", persist: true });

    const cold = buildGraphGeneration(coldRoot, { outDir: ".agent", persist: true });

    const incrementalImport = incremental.edges.find((e) => e.kind === "IMPORTS" && e.source === "file:src/caller.ts" && e.target === "file:src/util.ts");
    assert.ok(incrementalImport, "the aliased IMPORTS edge must appear once tsconfig paths exists");
    assert.equal(incrementalImport.resolved, true, "tsconfig paths alias must resolve once the config exists");
    assert.equal(incrementalImport.providerResolutions?.[0]?.resolutionTier, "tsconfig_paths");

    assert.deepEqual(nodeProjection(incremental), nodeProjection(cold), "nodes diverge between incremental (config added mid-stream) and cold (config from the start) builds");
    assert.deepEqual(edgeProjection(incremental), edgeProjection(cold), "edges diverge between incremental (config added mid-stream) and cold (config from the start) builds");
  } finally {
    rmSync(incrementalRoot, { recursive: true, force: true });
    rmSync(coldRoot, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// PROVIDER CHANGE: enabling the Tree-sitter provider between builds (no
// source change) must produce the same graph an equivalent from-scratch
// Tree-sitter-enabled build produces. Driven through the real CLI `build`
// command, the same production entry point freshness-regressions.test.mjs
// uses to prove Tree-sitter fact ownership.
// ---------------------------------------------------------------------------

function run(repo, args, env = {}) {
  return spawnSync(process.execPath, [CLI, ...args], {
    cwd: repo,
    encoding: "utf8",
    timeout: 30_000,
    env: { ...process.env, ...env },
  });
}

function dbCanonicalRows(repo) {
  const { openStore, closeStore } = requireStore();
  const db = openStore(join(repo, ".agent", "graph", "graph.db"));
  try {
    return {
      files: db.prepare("SELECT path,content_hash,language,provider,parse_status,error_node_count FROM files ORDER BY path").all(),
      symbols: db.prepare("SELECT id,kind,labels,name,qualified_name,path,confidence,evidence,extra FROM symbols ORDER BY id").all(),
      edges: db.prepare("SELECT id,kind,source,target,confidence,resolved,specifier,evidence,confidence_tier,extra FROM edges ORDER BY id").all(),
      owners: db.prepare("SELECT fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail FROM fact_owner ORDER BY provider_id,fact_kind,fact_id").all(),
    };
  } finally {
    closeStore(db);
  }
}

let storeModule;
function requireStore() {
  return storeModule;
}

test("provider equivalence: enabling Tree-sitter incrementally matches a cold Tree-sitter build", async () => {
  storeModule = await import("../src/graph/store-sqlite.mjs");
  const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

  const incrementalRoot = mkdtempSync(join(tmpdir(), "blueprint-provider-eq-incremental-"));
  const coldRoot = mkdtempSync(join(tmpdir(), "blueprint-provider-eq-cold-"));
  try {
    cpSync(FIXTURE, incrementalRoot, { recursive: true });
    cpSync(FIXTURE, coldRoot, { recursive: true });

    // Build 1 (incremental root): lexical-only, Tree-sitter disabled.
    const built1 = run(incrementalRoot, ["build", "--out", ".agent"], { BLUEPRINT_TREESITTER: "0" });
    assert.equal(built1.status, 0, built1.stderr);
    const providersBefore = dbCanonicalRows(incrementalRoot).owners.map((r) => r.provider_id);
    assert.ok(!providersBefore.includes("treesitter"), "Tree-sitter must be off for the first build");

    // Provider change: no source edit, just enable Tree-sitter and rebuild
    // the same repo incrementally (parse cache still warm from build 1).
    const built2 = run(incrementalRoot, ["build", "--out", ".agent"]);
    assert.equal(built2.status, 0, built2.stderr);

    // Cold: fresh checkout, Tree-sitter enabled from the very first build.
    const builtCold = run(coldRoot, ["build", "--out", ".agent"]);
    assert.equal(builtCold.status, 0, builtCold.stderr);

    const incremental = dbCanonicalRows(incrementalRoot);
    const cold = dbCanonicalRows(coldRoot);

    assert.ok(incremental.owners.some((r) => r.provider_id === "treesitter"), "Tree-sitter facts must appear once enabled incrementally");
    assert.deepEqual(incremental.files, cold.files, "files table diverges between incremental provider-enable and cold Tree-sitter build");
    assert.deepEqual(incremental.symbols, cold.symbols, "symbols table diverges between incremental provider-enable and cold Tree-sitter build");
    assert.deepEqual(incremental.edges, cold.edges, "edges table diverges between incremental provider-enable and cold Tree-sitter build");
    assert.deepEqual(incremental.owners, cold.owners, "fact_owner table diverges between incremental provider-enable and cold Tree-sitter build");
  } finally {
    rmSync(incrementalRoot, { recursive: true, force: true });
    rmSync(coldRoot, { recursive: true, force: true });
  }
});
