import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BLUEPRINT = path.resolve(HERE, "../..");
const CLI = path.join(BLUEPRINT, "scripts/blueprint.mjs");
const FIXTURE = path.join(BLUEPRINT, "evals/fixture-repos/typescript-commerce");
const SCHEMA = path.join(BLUEPRINT, "schemas/context-candidate-set.v1.schema.json");
import { validateJsonSchema } from "../python-test-runtime.mjs";
const workspaceSkip = false;

test("graph candidates CLI validates as ContextCandidateSet v1", { skip: workspaceSkip }, () => {
  const repo = path.join(os.tmpdir(), `blueprint-candidate-contract-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const generated = spawnSync(process.execPath, [CLI, "graph", "candidates", "--query", "placeOrder", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(generated.status, 0, generated.stderr || generated.stdout);
    const generatedPayload = JSON.parse(generated.stdout);
    assert.equal(generatedPayload.candidates.length > 0, true);
    const result = validateJsonSchema(JSON.parse(fs.readFileSync(SCHEMA, "utf8")), generatedPayload);
    assert.equal(result.valid, true, JSON.stringify(result.errors));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("candidate set applies real tiering: anchors reserved, exact flagged, overflow receipted", async () => {
  const { buildGraphGeneration, createContextCandidateSet } = await import("../../src/graph/static-provider.mjs");
  const repo = path.join(os.tmpdir(), `blueprint-tiering-${process.pid}-${Date.now()}`);
  fs.mkdirSync(repo, { recursive: true });
  try {
    spawnSync("git", ["init", "-q"], { cwd: repo });
    fs.writeFileSync(path.join(repo, "checkout.ts"), "export function placeOrder(){ return 1; }\nexport function placeOrderDraft(){ return 2; }\n");
    fs.writeFileSync(path.join(repo, "orders.ts"), "export function placeOrderRetry(){ return 3; }\nexport function orderQueue(){ return 4; }\n");
    fs.writeFileSync(path.join(repo, "anchor.ts"), "export function unrelatedThing(){ return 5; }\n");
    spawnSync("git", ["add", "-A"], { cwd: repo });
    const generation = buildGraphGeneration(repo, { outDir: ".agent", persist: true });

    // anchor a file the query does NOT match; force a small ceiling to create overflow.
    const cs = createContextCandidateSet(generation, { query: "placeOrder", anchors: ["anchor.ts"], maxCandidates: 3 });

    // Anchor reservation: the anchored file is present and protected, even though it
    // does not match "placeOrder".
    const protectedCands = cs.candidates.filter((c) => c.protected);
    assert.ok(protectedCands.length > 0, "anchored file must be a protected candidate");
    assert.ok(protectedCands.every((c) => c.sourceRef.startsWith("anchor.ts")), "protected candidates are the anchored file");

    // Tier order: protected sorts before exact before lexical.
    assert.equal(cs.candidates[0].protected, true, "protected candidate ranks first");

    // Exact vs lexical is real, not hardcoded true.
    assert.ok(cs.candidates.some((c) => c.exact === true), "an exact name match is flagged exact");
    assert.ok(cs.candidates.some((c) => c.exact === false), "a non-exact match is flagged not-exact (was hardcoded true)");

    // Omissions are real receipts, not an always-empty array.
    assert.ok(cs.candidates.length <= 3, "ceiling enforced");
    assert.ok(cs.omissions.length > 0, "overflow candidates are receipted, not silently dropped");
    assert.ok(cs.omissions.every((o) => typeof o.reason === "string" && o.reason.length > 0), "each omission carries a reason");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("pinned generation answers from the store alone — no source-tree walk", () => {
  // Latency contract for the prompt-path caller (membrane federation, 350ms
  // budget): with --expected-generation, candidates must not re-derive
  // staleness by walking/hashing sources. Proven structurally: delete every
  // source file after build; only the store remains. A tree walk would see a
  // stale/missing repo and refuse — the pinned door must still answer.
  const repo = path.join(os.tmpdir(), `blueprint-pinned-door-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], {
      cwd: repo, encoding: "utf8",
    });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const manifest = spawnSync(process.execPath, [CLI, "graph", "manifest", "--out", ".agent"], {
      cwd: repo, encoding: "utf8",
    });
    assert.equal(manifest.status, 0, manifest.stderr || manifest.stdout);
    const generation = JSON.parse(manifest.stdout).generationId;
    for (const entry of fs.readdirSync(repo)) {
      if (entry !== ".agent") fs.rmSync(path.join(repo, entry), { recursive: true, force: true });
    }
    const pinned = spawnSync(process.execPath, [CLI, "graph", "candidates", "--query", "placeOrder",
      "--out", ".agent", "--expected-generation", generation], { cwd: repo, encoding: "utf8" });
    assert.equal(pinned.status, 0, pinned.stderr || pinned.stdout);
    assert.equal(JSON.parse(pinned.stdout).candidates.length > 0, true);
    const wrongPin = spawnSync(process.execPath, [CLI, "graph", "candidates", "--query", "placeOrder",
      "--out", ".agent", "--expected-generation", "xxh128:" + "f".repeat(32)], { cwd: repo, encoding: "utf8" });
    assert.notEqual(wrongPin.status, 0, "a stale pin must fail closed");
    assert.match(wrongPin.stderr || wrongPin.stdout, /graph_generation_changed/);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
