// D50: build N-2 / N-1 fixture stores for migration testing.
//
// Materialise real N-1/N-2 stores through current save code at capped schemas.
// the current line without data loss. SQLite stores are binary, so the
// fixtures are MATERIALISED BY THIS SCRIPT into a temp directory rather than
// committed as blobs: `node fixtures/stores/build-stores.mjs <outDir>` writes
// v15.db and v14.db, each seeded with one deterministic generation. Tests
// then open those files with openStore() (which migrates to the latest line)
// and assert every row survived.
//
// The seed is generated with the REAL saveGeneration() at a capped schema
// version (openStore upToVersion), so fixture stores are byte-identical to
// what a real past-version blueprint would have written — not hand-rolled tables.

import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { closeStore, openStore, saveGeneration } from "../../src/graph/store-sqlite.mjs";

function fileNode(path, hash, language = "typescript") {
  return { id: `file:${path}`, kind: "file", name: path, path, labels: ["file"], language, confidence: 1, evidence: [{ contentHash: hash }], factProvider: { id: "lexical", version: "9.9.9" } };
}

function symNode(path, name, kinds) {
  return { id: `symbol:${path}::${name}`, kind: kinds[0], name, qualifiedName: `${path}::${name}`, path, labels: kinds, confidence: 1, evidence: [], factProvider: { id: "lexical", version: "9.9.9" } };
}

function seedGeneration(version) {
  // Version-tagged source so a mis-wired test cannot pass by accident: the
  // symbol names differ per version, and every assertion checks its own tag.
  const prefix = `v${version}`;
  return {
    schemaVersion: version,
    provider: { id: "blueprint-treesitter", version: "9.9.9" },
    manifest: { generationId: `gen-${prefix}`, complete: true, repo: "fixture-stores" },
    repoRoot: "/fixture/stores",
      nodes: [
        fileNode("a.ts", `h-a-${prefix}`),
        fileNode("b.ts", `h-b-${prefix}`),
        symNode("a.ts", `${prefix}Alpha`, ["Function"]),
        ...(version >= 15 ? [{ id: "comment:a.ts:1", kind: "comment", labels: ["Comment"], name: null, qualifiedName: null, path: "a.ts", confidence: 1, evidence: [], text: "// v15 interleave", factProvider: { id: "treesitter", version: "9.9.9" } }] : []),
        symNode("b.ts", `${prefix}Beta`, ["Class"]),
    ],
    edges: [
      { id: `edge:${prefix}:import:b->a`, kind: "IMPORT", source: "file:b.ts", target: "file:a.ts", confidence: 1, resolved: true, evidence: [] },
    ],
    fileReports: [
      { path: "a.ts", language: "typescript", provider: "blueprint-treesitter", parseStatus: "ok", errorNodeCount: 0 },
      { path: "b.ts", language: "typescript", provider: "blueprint-treesitter", parseStatus: "ok", errorNodeCount: 0 },
    ],
  };
}

/**
 * Materialise the N-1 (16) and N-2 (15) fixture stores into `outDir`.
 * Returns the list of written store files. Deterministic: the same outDir
 * always yields the same seeds and schema versions.
 */
export function buildFixtureStores(outDir) {
  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
  const versions = [16, 15];
  return versions.map((version) => {
    const dbPath = join(outDir, `v${version}.db`);
    rmSync(dbPath, { force: true });
    for (const suffix of ["-wal", "-shm"]) {
      const sidecar = dbPath + suffix;
      if (existsSync(sidecar)) rmSync(sidecar, { force: true });
    }
    const db = openStore(dbPath, { upToVersion: version });
    saveGeneration(db, seedGeneration(version));
    if (version === 16) {
      db.exec(`DELETE FROM provider_ranks;
        INSERT INTO provider_ranks(provider_id,rank) VALUES ('blueprint-static',7),('lexical',3),('blueprint-treesitter',9),('custom',20);
        UPDATE files SET node_ordinal=10;
        UPDATE symbols SET node_ordinal=CASE WHEN path='a.ts' THEN 30 ELSE 40 END;
        UPDATE annotation_nodes SET node_ordinal=50;
        UPDATE symbols SET extra=json_set(COALESCE(extra,'{}'),'$.factProvider.id','custom') WHERE path='b.ts';`);
      db.prepare(`INSERT INTO fact_owner(fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail,generation_id,repo_root)
        VALUES ('comment:a.ts:1','node','a.ts','xxh128:h-a-v16','blueprint-treesitter','9.9.9','structural','comment','gen-v16','/fixture/stores'),
          ('doc:claim:v16','node','docs/v16.md','xxh128:doc','doctruth','1','doc','claim','gen-v16','/fixture/stores')`).run();
    }
    closeStore(db);
    return dbPath;
  });
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.url)) {
  const outDir = process.argv[2] ?? join(".", ".fixture-stores");
  for (const path of buildFixtureStores(outDir)) console.log(`fixture store ready: ${path}`);
}
