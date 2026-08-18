// Test-only helpers for reading and mutating the graph store's envelope.
//
// These live here rather than as exports on store-sqlite.mjs on purpose: a
// production module should not carry functions whose only callers are tests —
// that is exactly the shipped-but-unwired pattern that let the store sit
// write-only for a day without anyone noticing.
//
// Several tests need to corrupt or age a manifest to prove the freshness and
// provider-compatibility gates actually fire. Before the single-store
// migration they did that by rewriting .agent/graph/manifest.json; the
// equivalent now is updating the `generation` envelope row.

import { join, resolve, dirname } from "node:path";
import { mkdirSync } from "node:fs";
import { openStore, closeStore, saveGeneration } from "../src/graph/store-sqlite.mjs";

// `resolve`, not `join` — several tests pass an ABSOLUTE outDir (a temp dir
// outside the repo), and production code resolves the same way. `join` would
// silently concatenate it onto repoRoot and open an empty store somewhere else,
// which reads as "no graph" rather than as an error.
function storePath(repoRoot, outDir = ".agent") {
  return join(resolve(repoRoot, outDir), "graph", "graph.db");
}

/** Read one envelope key (default: the manifest) from a repo's graph store. */
export function readEnvelope(repoRoot, key = "manifest", outDir = ".agent") {
  const db = openStore(storePath(repoRoot, outDir));
  try {
    const row = db.prepare("SELECT value FROM generation WHERE key = ?").get(key);
    return row ? JSON.parse(row.value) : null;
  } finally {
    closeStore(db);
  }
}

/** Overwrite one envelope key, so a test can age or corrupt a stored manifest. */
export function writeEnvelope(repoRoot, key, value, outDir = ".agent") {
  const db = openStore(storePath(repoRoot, outDir));
  try {
    db.prepare(
      "INSERT INTO generation (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    ).run(key, JSON.stringify(value));
  } finally {
    closeStore(db);
  }
}

/** Convenience: mutate the stored manifest through a callback. */
export function mutateManifest(repoRoot, mutate, outDir = ".agent") {
  const manifest = readEnvelope(repoRoot, "manifest", outDir);
  mutate(manifest);
  writeEnvelope(repoRoot, "manifest", manifest, outDir);
  return manifest;
}

/**
 * Write a hand-built generation into a repo's store.
 *
 * Tests that used to fabricate a small .agent/graph/graph.json to make an
 * adapter believe a graph exists need this now that the store is the only
 * format. Creates the directory tree, so a caller can seed a bare temp repo.
 */
export function seedStore(repoRoot, generation, outDir = ".agent") {
  const target = storePath(repoRoot, outDir);
  mkdirSync(dirname(target), { recursive: true });
  const db = openStore(target);
  try {
    saveGeneration(db, { schemaVersion: 1, nodes: [], edges: [], ...generation });
  } finally {
    closeStore(db);
  }
  return target;
}

export { storePath };
