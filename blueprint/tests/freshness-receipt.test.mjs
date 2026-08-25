// Query-time freshness receipt (§17.2.2). Two properties are load-bearing and
// tested on SEPARATE paths, per the canon warning that conflating them breaks
// a downstream consumer:
//
//   1. `changed_since_generation` is a TRUTHFUL SUCCESSFUL result — the index
//      is coherent, the worktree moved on. It must never be treated as, or
//      reported alongside, an error.
//   2. A generation mismatch FAILS CLOSED at every freshness state,
//      including `fresh`. Pinning a generation and being served a different
//      one is unsafe even when the served generation is itself fresh.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { closeStore, openStore, readManifestEnvelope, saveGeneration } from "../src/graph/store-sqlite.mjs";
import { gitSourceObservation } from "../src/graph/git-source-observation.mjs";
import {
  assertGenerationCoherence,
  buildFreshnessReceipt,
  evaluateFreshness,
  FRESHNESS_RECEIPT_SCHEMA,
  generationFreshnessBasis,
  observeCurrentVcsState,
} from "../src/graph/freshness-receipt.mjs";

function git(root, args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function withGitRepo(fn) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-freshness-receipt-"));
  try {
    git(root, ["init", "--quiet"]);
    git(root, ["config", "user.email", "test@example.invalid"]);
    git(root, ["config", "user.name", "Test"]);
    writeFileSync(join(root, "app.js"), "export const value = 1;\n");
    git(root, ["add", "app.js"]);
    git(root, ["commit", "--quiet", "-m", "fixture"]);
    return fn(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function sealedStore(sourceObservation, { generationId = "gen-1", manifestDigest = "sha256:fixture" } = {}) {
  const db = openStore(":memory:");
  saveGeneration(db, {
    manifest: { generationId, manifestDigest, counts: { nodes: 0, edges: 0 } },
    provider: { id: "lexical", version: "1" },
    nodes: [],
    edges: [],
    ...(sourceObservation !== undefined ? { sourceObservation } : {}),
  });
  return db;
}

// ---------------------------------------------------------------------------
// Pure freshness axis
// ---------------------------------------------------------------------------

test("evaluateFreshness: fresh when current matches indexed revision + fingerprint exactly", () => {
  const generation = { indexed_revision: "abc123", indexed_worktree_fingerprint: "fp-1" };
  const current = { available: true, vcs_revision: "abc123", worktree_fingerprint: "fp-1" };
  assert.equal(evaluateFreshness(generation, current), "fresh");
});

test("evaluateFreshness: changed_since_generation on revision drift — a truthful success, not a failure", () => {
  const generation = { indexed_revision: "abc123", indexed_worktree_fingerprint: "fp-1" };
  const current = { available: true, vcs_revision: "def456", worktree_fingerprint: "fp-2" };
  assert.equal(evaluateFreshness(generation, current), "changed_since_generation");
});

test("evaluateFreshness: changed_since_generation on same revision but dirty-overlay drift (fingerprint differs)", () => {
  const generation = { indexed_revision: "abc123", indexed_worktree_fingerprint: "fp-clean" };
  const current = { available: true, vcs_revision: "abc123", worktree_fingerprint: "fp-dirty" };
  assert.equal(evaluateFreshness(generation, current), "changed_since_generation");
});

test("evaluateFreshness: unknown when the generation carries no indexed basis", () => {
  const current = { available: true, vcs_revision: "abc123", worktree_fingerprint: "fp-1" };
  assert.equal(evaluateFreshness({ indexed_revision: null, indexed_worktree_fingerprint: null }, current), "unknown");
});

test("evaluateFreshness: unavailable when current-state observation itself failed — never silently 'fresh'", () => {
  const generation = { indexed_revision: "abc123", indexed_worktree_fingerprint: "fp-1" };
  assert.equal(evaluateFreshness(generation, { available: false }), "unavailable");
  // unavailable wins even when the generation basis is ALSO missing —
  // "no git" is reported honestly, not laundered into "unknown".
  assert.equal(evaluateFreshness({ indexed_revision: null }, { available: false }), "unavailable");
});

test("generationFreshnessBasis reads indexed_revision/fingerprint from the envelope's sourceObservation", () => {
  assert.deepEqual(
    generationFreshnessBasis({ sourceObservation: { head: "abc", dirty: false, statusDigest: "fp-1" } }),
    { indexed_revision: "abc", indexed_worktree_fingerprint: "fp-1" },
  );
  assert.deepEqual(generationFreshnessBasis(null), { indexed_revision: null, indexed_worktree_fingerprint: null });
  assert.deepEqual(generationFreshnessBasis({ sourceObservation: null }), { indexed_revision: null, indexed_worktree_fingerprint: null });
});

test("observeCurrentVcsState reports available:false, not a thrown error, when git itself is unavailable", () => {
  const state = observeCurrentVcsState("/nonexistent/not-a-repo", { observe: () => null });
  assert.deepEqual(state, { available: false, vcs_revision: null, dirty: null, worktree_fingerprint: null });
});

// ---------------------------------------------------------------------------
// buildFreshnessReceipt — real git, real store, "actually observe" not "time since"
// ---------------------------------------------------------------------------

test("buildFreshnessReceipt: fresh immediately after indexing a clean commit", () => {
  withGitRepo((root) => {
    const observation = gitSourceObservation(root);
    const db = sealedStore(observation);
    try {
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.schema, FRESHNESS_RECEIPT_SCHEMA);
      assert.equal(receipt.freshness, "fresh");
      assert.equal(receipt.generation.indexed_revision, observation.head);
      assert.equal(receipt.current.vcs_revision, observation.head);
      assert.equal(receipt.current.dirty, false);
    } finally {
      closeStore(db);
    }
  });
});

test("buildFreshnessReceipt: changed_since_generation after an uncommitted edit — the current-state check actually observes the worktree", () => {
  withGitRepo((root) => {
    const observation = gitSourceObservation(root);
    const db = sealedStore(observation);
    try {
      writeFileSync(join(root, "app.js"), "export const value = 999;\n");
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "changed_since_generation");
      // Same commit — this is a worktree-content change, not a HEAD move.
      assert.equal(receipt.current.vcs_revision, observation.head);
      assert.equal(receipt.current.dirty, true);
      assert.notEqual(receipt.current.worktree_fingerprint, receipt.generation.indexed_worktree_fingerprint);
    } finally {
      closeStore(db);
    }
  });
});

test("buildFreshnessReceipt: changed_since_generation after a new commit moves HEAD", () => {
  withGitRepo((root) => {
    const observation = gitSourceObservation(root);
    const db = sealedStore(observation);
    try {
      writeFileSync(join(root, "app.js"), "export const value = 2;\n");
      git(root, ["commit", "--quiet", "-am", "second commit"]);
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "changed_since_generation");
      assert.notEqual(receipt.current.vcs_revision, observation.head);
      assert.equal(receipt.generation.indexed_revision, observation.head);
    } finally {
      closeStore(db);
    }
  });
});

test("buildFreshnessReceipt: unknown for a generation sealed without a source observation (e.g. non-git worktree at index time)", () => {
  withGitRepo((root) => {
    const db = sealedStore(undefined);
    try {
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "unknown");
      assert.equal(receipt.generation.indexed_revision, null);
    } finally {
      closeStore(db);
    }
  });
});

test("buildFreshnessReceipt: unavailable when the repo root is not (or no longer) a git worktree", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-freshness-nongit-"));
  try {
    const observation = { head: "deadbeef", dirty: false, statusDigest: "fp-x" };
    const db = sealedStore(observation);
    try {
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "unavailable");
      assert.equal(receipt.current.vcs_revision, null);
    } finally {
      closeStore(db);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// Generation coherence — an INDEPENDENT axis from freshness (§17.2.2)
// ---------------------------------------------------------------------------

test("assertGenerationCoherence: no-op when nothing is pinned, or the pin matches", () => {
  assert.doesNotThrow(() => assertGenerationCoherence({ pinnedGenerationId: null, servedGenerationId: "gen-1" }));
  assert.doesNotThrow(() => assertGenerationCoherence({ pinnedGenerationId: "gen-1", servedGenerationId: "gen-1" }));
});

test("assertGenerationCoherence: changed_since_generation is NOT, by itself, a mismatch", () => {
  withGitRepo((root) => {
    const observation = gitSourceObservation(root);
    const db = sealedStore(observation, { generationId: "gen-pinned" });
    try {
      writeFileSync(join(root, "app.js"), "export const value = 42;\n");
      const envelope = readManifestEnvelope(db);
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "changed_since_generation");
      // A consumer pinned to the generation that is STILL being served must
      // not have this staleness-only signal treated as a mismatch.
      assert.doesNotThrow(() => assertGenerationCoherence({
        pinnedGenerationId: "gen-pinned",
        servedGenerationId: envelope.generationId,
      }));
    } finally {
      closeStore(db);
    }
  });
});

test("assertGenerationCoherence: FAILS CLOSED on a generation mismatch even when freshness reports fresh", () => {
  withGitRepo((root) => {
    const observation = gitSourceObservation(root);
    // Sealed as "gen-current" — worktree exactly matches what was indexed.
    const db = sealedStore(observation, { generationId: "gen-current" });
    try {
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "fresh", "the served generation is genuinely fresh");
      // A consumer that pinned an EARLIER generation (e.g. a rebuild ran
      // between its recall and this query, reindexing the identical clean
      // tree into a new generationId) must still hard-fail: fresh is not a
      // license to skip the coherence check.
      assert.throws(
        () => assertGenerationCoherence({ pinnedGenerationId: "gen-earlier", servedGenerationId: receipt.generationId }),
        (error) => {
          assert.equal(error.code, "generation_mismatch");
          assert.deepEqual(error.details, { pinnedGenerationId: "gen-earlier", servedGenerationId: "gen-current" });
          return true;
        },
      );
    } finally {
      closeStore(db);
    }
  });
});

test("assertGenerationCoherence: fails closed at unknown and unavailable freshness too, not only fresh/changed", () => {
  withGitRepo((root) => {
    const db = sealedStore(undefined, { generationId: "gen-x" }); // unknown freshness
    try {
      const receipt = buildFreshnessReceipt(db, root);
      assert.equal(receipt.freshness, "unknown");
      assert.throws(
        () => assertGenerationCoherence({ pinnedGenerationId: "gen-other", servedGenerationId: receipt.generationId }),
        (error) => error.code === "generation_mismatch",
      );
    } finally {
      closeStore(db);
    }
  });
});
