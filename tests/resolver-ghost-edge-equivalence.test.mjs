import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { buildGraphGeneration } from "../graph/static-provider.mjs";

const corpus = JSON.parse(readFileSync(join(import.meta.dirname, "../evals/equivalence/ghost-edge-corpus-v1.json"), "utf8"));

function makeRepo(prefix, files) {
  const root = mkdtempSync(join(tmpdir(), prefix));
  execFileSync("git", ["init", "-q"], { cwd: root });
  for (const [path, content] of Object.entries(files)) {
    const destination = join(root, path);
    // Corpus paths are flat by design; retain this guard if a later fixture nests one.
    if (dirname(destination) !== root) throw new Error(`nested corpus path requires explicit directory setup: ${path}`);
    writeFileSync(destination, content);
  }
  execFileSync("git", ["add", "-A"], { cwd: root });
  return root;
}

function applyMutation(root, mutation) {
  for (const path of mutation.delete ?? []) unlinkSync(join(root, path));
  for (const [path, content] of Object.entries(mutation.write ?? {})) writeFileSync(join(root, path), content);
  execFileSync("git", ["add", "-A"], { cwd: root });
}

function edgeProjection(generation) {
  return generation.edges.map(({ id, kind, source, target, resolved, specifier, reason, confidence, confidenceTier }) => ({
    id, kind, source, target, resolved, specifier, reason: reason ?? null, confidence, confidenceTier,
  }));
}

function canonicalOutput(generation) {
  // repoRoot is host-specific; all remaining fields are serialized build output.
  const { repoRoot, ...output } = generation;
  return Buffer.from(JSON.stringify(output));
}

function coldComparable(generation) {
  return {
    generationId: generation.manifest.generationId,
    nodes: generation.nodes,
    edges: edgeProjection(generation),
  };
}

for (const fixture of corpus.cases) {
  test(`resolver equivalence: ${fixture.id}`, () => {
    const incrementalRoot = makeRepo(`cortex-ghost-${fixture.id}-incremental-`, fixture.initial);
    const coldRoot = makeRepo(`cortex-ghost-${fixture.id}-cold-`, fixture.initial);
    try {
      const initial = buildGraphGeneration(incrementalRoot, { outDir: ".agent", persist: true });
      const noOp = buildGraphGeneration(incrementalRoot, { outDir: ".agent", persist: true });
      assert.deepEqual(edgeProjection(initial), fixture.expectedInitialEdges, "ordered initial edge projection changed from frozen corpus");
      assert.deepEqual(canonicalOutput(noOp), canonicalOutput(initial), "no-op rebuild output must be byte-identical");
      assert.equal(noOp.manifest.generationId, initial.manifest.generationId, "no-op rebuild changed generation identity");

      applyMutation(incrementalRoot, fixture.mutation);
      applyMutation(coldRoot, fixture.mutation);
      const incremental = buildGraphGeneration(incrementalRoot, { outDir: ".agent", persist: true });
      const cold = buildGraphGeneration(coldRoot, { outDir: ".agent", persist: true });

      assert.deepEqual(edgeProjection(incremental), fixture.expectedEdges, "ordered edge projection changed from frozen corpus");
      assert.deepEqual(coldComparable(incremental), coldComparable(cold), "warm resolver output diverged from cold rebuild");
    } finally {
      rmSync(incrementalRoot, { recursive: true, force: true });
      rmSync(coldRoot, { recursive: true, force: true });
    }
  });
}
